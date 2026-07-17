use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt;
use std::path::Path;

use allocative::Allocative;
use starlark::any::ProvidesStaticType;
use starlark::environment::FrozenModule;
use starlark::environment::Globals;
use starlark::environment::GlobalsBuilder;
use starlark::environment::Module;
use starlark::eval::Evaluator;
use starlark::eval::ReturnFileLoader;
use starlark::starlark_module;
use starlark::starlark_simple_value;
use starlark::syntax::AstModule;
use starlark::syntax::Dialect;
use starlark::syntax::DialectTypes;
use starlark::values::FrozenHeapName;
use starlark::values::Heap;
use starlark::values::NoSerialize;
use starlark::values::StarlarkValue;
use starlark::values::Value;
use starlark::values::list::UnpackList;
use starlark::values::none::NoneType;
use starlark_derive::starlark_value;

use crate::config::Config;
use crate::config::JobConfig;
use crate::config::JobId;
use crate::config::JobState;
use crate::config::NeedConfig;
use crate::config::PipelineConfig;
use crate::config::PipelineId;
use crate::config::SecretConfig;
use crate::config::StepConfig;
use crate::config::StepId;
use crate::config::WhenConfig;
use crate::graph::Graph;
use crate::graph::reachable;

pub type Env = HashMap<String, String>;
pub type JobRegistry = HashMap<JobId, JobConfig>;

/// A collector of all jobs and pipelines constructed during evaluation.
/// This is used as the `extra` field of the Evaluator, so that the globals can
/// push jobs and pipelines into it. `extra` is an immutable reference, so we
/// use RefCell to allow interior mutability (borrowing is checked at runtime
/// instead of compile time).
#[derive(Debug, ProvidesStaticType)]
pub struct Collector {
    job_registry: RefCell<JobRegistry>,
    job_needs: RefCell<Graph<JobId>>,
    job_needed_bys: RefCell<Graph<JobId>>,
    pipelines: RefCell<Vec<PipelineConfig>>,
    globals: Globals,
}

impl Collector {
    pub fn new(env: &Env) -> Self {
        let globals = {
            let mut builder = GlobalsBuilder::standard().with(predefined_primitives);
            env.iter().for_each(|(k, v)| builder.set(k, v.clone()));
            builder.build()
        };

        Self {
            job_registry: RefCell::new(HashMap::new()),
            pipelines: RefCell::new(Vec::new()),
            job_needs: RefCell::new(HashMap::new()),
            job_needed_bys: RefCell::new(HashMap::new()),
            globals,
        }
    }

    /// Evaluate one Starlark file, recursively resolving its load() statements.
    /// Each loaded module is frozen before being made available to its importer.
    pub fn evaluate(&self, path: &Path) -> anyhow::Result<FrozenModule> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("failed to read file [{}]:\n{}", path.display(), e))?;

        // Enable f-strings, type annotations, and top-level statements (for/if at
        // module level) used in pipeline config files.
        let dialect = Dialect {
            enable_f_strings: true,
            enable_types: DialectTypes::Enable,
            enable_top_level_stmt: true,
            ..Dialect::Standard
        };

        let ast = AstModule::parse(&path.to_string_lossy(), content, &dialect).map_err(|e| {
            anyhow::anyhow!("failed to parse file contents [{}]:\n{}", path.display(), e)
        })?;

        // Collect all load() module IDs before the AST is consumed by eval_module.
        let load_ids: Vec<String> = ast
            .loads()
            .into_iter()
            .map(|l| l.module_id.to_owned())
            .collect();

        // Recursively resolve and freeze each loaded dependency.
        let loader_base = path.parent().unwrap_or(Path::new("."));

        let mut modules: Vec<(String, FrozenModule)> = Vec::new();
        for id in &load_ids {
            let dep_path = loader_base.join(id.as_str());

            let frozen = self.evaluate(&dep_path)?;
            modules.push((id.clone(), frozen));
        }

        let modules_map = modules.iter().map(|(k, v)| (k.as_str(), v)).collect();

        let mut loader = ReturnFileLoader {
            modules: &modules_map,
        };

        let result = Module::with_temp_heap(|module| -> anyhow::Result<FrozenModule> {
            {
                let mut eval = Evaluator::new(&module);
                eval.set_loader(&mut loader);
                eval.extra = Some(self);
                eval.eval_module(ast, &self.globals)
                    .map_err(|e| anyhow::anyhow!("failed to evaluate module:\n{}", e))?;
            }

            let module = module.freeze_named(FrozenHeapName::User(Box::new(
                path.to_string_lossy().into_owned(),
            )))?;

            Ok(module)
        });

        result.map_err(|e| anyhow::anyhow!("{}: {}", path.display(), e))
    }

    fn collect_job(&self, job: &JobConfig) {
        self.job_registry
            .borrow_mut()
            .insert(job.job_id, job.clone());

        let mut job_needs = self.job_needs.borrow_mut();
        let mut job_needed_bys = self.job_needed_bys.borrow_mut();

        for need in &job.needs {
            job_needs.entry(job.job_id).or_default().insert(need.job_id);

            job_needed_bys
                .entry(need.job_id)
                .or_default()
                .insert(job.job_id);
        }
    }

    fn collect_pipeline(&self, pipeline: &PipelineConfig) {
        self.pipelines.borrow_mut().push(pipeline.clone());
    }
}

impl TryInto<Config> for Collector {
    type Error = anyhow::Error;

    fn try_into(self) -> Result<Config, Self::Error> {
        let pipelines = self.pipelines.into_inner();
        let job_registry = self.job_registry.into_inner();
        let job_needs = self.job_needs.into_inner();
        let job_needed_bys = self.job_needed_bys.into_inner();

        // Prune jobs and job_graph to only include jobs reachable from the DAG
        // formed by the pipeline targets.
        let reachable_job_ids = {
            let mut reachable_job_ids = HashSet::new();

            pipelines
                .iter()
                .try_for_each(|pipeline| -> anyhow::Result<()> {
                    let job_ids = pipeline
                        .needs
                        .iter()
                        .map(|need| need.job_id)
                        .collect::<HashSet<JobId>>();

                    // Forwards pass to capture reachable jobs from the pipeline's needs.
                    let job_ids = reachable(&job_ids, &job_needs).map_err(|cycle| {
                        anyhow::anyhow!(
                            "Pipeline [{}] has a cycle in its job dependencies: {:?}",
                            pipeline.name,
                            cycle
                        )
                    })?;

                    // Backwards pass to capture all jobs that need reachable jobs.
                    let job_ids = reachable(&job_ids, &job_needed_bys).map_err(|cycle| {
                        anyhow::anyhow!(
                            "Pipeline [{}] has a cycle in its job dependencies: {:?}",
                            pipeline.name,
                            cycle
                        )
                    })?;

                    reachable_job_ids.extend(job_ids);

                    Ok(())
                })?;

            reachable_job_ids
        };

        let jobs = job_registry
            .into_iter()
            .filter(|(job_id, _)| reachable_job_ids.contains(job_id))
            .map(|(_, job)| job)
            .collect::<Vec<JobConfig>>();

        let config = Config {
            pipelines: pipelines,
            jobs: jobs,
        };

        Ok(config)
    }
}

/// A Starlark value that wraps a [`Secret`].
#[derive(Debug, ProvidesStaticType, NoSerialize, Allocative)]
struct SecretVal {
    #[allocative(skip)]
    inner: SecretConfig,
}

impl fmt::Display for SecretVal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Secret(name={:?}, key={:?})",
            self.inner.name, self.inner.key
        )
    }
}

starlark_simple_value!(SecretVal);

#[starlark_value(type = "Secret")]
impl<'v> StarlarkValue<'v> for SecretVal {}

/// A Starlark value that wraps a [`Step`].
#[derive(Debug, ProvidesStaticType, NoSerialize, Allocative)]
struct StepVal {
    #[allocative(skip)]
    inner: StepConfig,
}

impl fmt::Display for StepVal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Step(name={:?})", self.inner.name)
    }
}

starlark_simple_value!(StepVal);

#[starlark_value(type = "Step")]
impl<'v> StarlarkValue<'v> for StepVal {}

/// A Starlark value that wraps a [`Job`].
#[derive(Debug, ProvidesStaticType, NoSerialize, Allocative)]
struct JobVal {
    #[allocative(skip)]
    inner: JobConfig,
}

impl fmt::Display for JobVal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Job(name={:?}, id={})",
            self.inner.name, self.inner.job_id
        )
    }
}

starlark_simple_value!(JobVal);

#[starlark_value(type = "Job")]
impl<'v> StarlarkValue<'v> for JobVal {
    fn get_attr(&self, attribute: &str, heap: Heap<'v>) -> Option<Value<'v>> {
        let state = match attribute {
            "completed" => JobState::Completed,
            "failed" => JobState::Failed,
            "skipped" => JobState::Skipped,
            _ => return None,
        };
        Some(heap.alloc(NeedVal {
            inner: NeedConfig {
                job_id: self.inner.job_id,
                state,
            },
        }))
    }

    fn has_attr(&self, attribute: &str, _heap: Heap<'v>) -> bool {
        matches!(attribute, "completed" | "failed" | "skipped")
    }

    fn dir_attr(&self) -> Vec<String> {
        vec![
            "completed".to_owned(),
            "failed".to_owned(),
            "skipped".to_owned(),
        ]
    }
}

/// A Starlark value that wraps a [`Need`].
#[derive(Debug, ProvidesStaticType, NoSerialize, Allocative)]
struct NeedVal {
    #[allocative(skip)]
    inner: NeedConfig,
}

impl fmt::Display for NeedVal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Need(job_id={}, state={:?})",
            self.inner.job_id, self.inner.state
        )
    }
}

starlark_simple_value!(NeedVal);

#[starlark_value(type = "Need")]
impl<'v> StarlarkValue<'v> for NeedVal {}

/// A Starlark value that wraps a [`Rule`].
#[derive(Debug, ProvidesStaticType, NoSerialize, Allocative)]
struct WhenVal {
    #[allocative(skip)]
    inner: WhenConfig,
}

impl fmt::Display for WhenVal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "When({:?})", self.inner)
    }
}

starlark_simple_value!(WhenVal);

#[starlark_value(type = "When")]
impl<'v> StarlarkValue<'v> for WhenVal {}

/// A Starlark module that provides predefined primitives for defining
/// pipeline configuration.
#[starlark_module]
pub fn predefined_primitives(builder: &mut GlobalsBuilder) {
    fn Secret(
        #[starlark(require = named)] name: &str,
        #[starlark(require = named)] key: &str,
    ) -> starlark::Result<SecretVal> {
        Ok(SecretVal {
            inner: SecretConfig {
                name: name.to_owned(),
                key: key.to_owned(),
            },
        })
    }

    fn Step<'v>(
        #[starlark(require = named)] command: &str,
        #[starlark(require = named)]
        #[starlark(default = "")]
        name: &str,
        #[starlark(require = named)]
        #[starlark(default = NoneType)]
        secrets: Value<'v>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> starlark::Result<StepVal> {
        let mut secrets_vec: Vec<SecretConfig> = Vec::new();
        if !secrets.is_none() {
            for item in secrets.iterate(eval.heap())? {
                let s = SecretVal::from_value(item).ok_or_else(|| {
                    starlark::Error::new_other(anyhow::anyhow!(
                        "Step.secrets: expected Secret, got {}",
                        item.get_type()
                    ))
                })?;
                secrets_vec.push(s.inner.clone());
            }
        }
        let step_id = StepId::now_v7();
        let name = if name.is_empty() {
            format!("step-{}", step_id)
        } else {
            name.to_owned()
        };
        Ok(StepVal {
            inner: StepConfig {
                step_id,
                name: name,
                command: command.to_owned(),
                secrets: secrets_vec,
            },
        })
    }

    fn Job<'v>(
        #[starlark(require = named)] steps: Value<'v>,
        #[starlark(require = named)]
        #[starlark(default = "")]
        name: &str,
        #[starlark(require = named)]
        #[starlark(default = NoneType)]
        needs: Value<'v>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> starlark::Result<JobVal> {
        let steps = {
            let mut steps_vec: Vec<StepConfig> = Vec::new();
            for item in steps.iterate(eval.heap())? {
                let s = StepVal::from_value(item).ok_or_else(|| {
                    starlark::Error::new_other(anyhow::anyhow!(
                        "Job.steps: expected Step, got {}",
                        item.get_type()
                    ))
                })?;
                steps_vec.push(s.inner.clone());
            }
            steps_vec
        };

        let needs = {
            let mut needs_set: HashSet<NeedConfig> = HashSet::new();
            if !needs.is_none() {
                for item in needs.iterate(eval.heap())? {
                    let need = if let Some(job) = JobVal::from_value(item) {
                        NeedConfig {
                            job_id: job.inner.job_id,
                            state: JobState::Completed,
                        }
                    } else if let Some(need) = NeedVal::from_value(item) {
                        need.inner
                    } else {
                        return Err(starlark::Error::new_other(anyhow::anyhow!(
                            "Job.needs: expected Need or Job, got {}",
                            item.get_type()
                        )));
                    };

                    needs_set.insert(need);
                }
            }

            needs_set.into_iter().collect::<Vec<NeedConfig>>()
        };

        let job = {
            let job_id = JobId::now_v7();

            let name = if name.is_empty() {
                format!("job-{}", job_id)
            } else {
                name.to_owned()
            };

            JobConfig {
                job_id,
                name,
                steps,
                needs,
            }
        };

        eval.extra
            .ok_or_else(|| {
                starlark::Error::new_other(anyhow::anyhow!("no Collector in eval.extra"))
            })?
            .downcast_ref::<Collector>()
            .ok_or_else(|| {
                starlark::Error::new_other(anyhow::anyhow!("eval.extra is not a Collector"))
            })?
            .collect_job(&job);

        Ok(JobVal { inner: job })
    }

    /// A Starlark function to construct a Push object.
    /// Returns a `Rule` Starlark value that can be used in a Pipeline's `when` list.
    fn Push<'v>(
        #[starlark(require = named)] include_branches: Option<UnpackList<String>>,
        #[starlark(require = named)] exclude_branches: Option<UnpackList<String>>,
        #[starlark(require = named)] include_tags: Option<UnpackList<String>>,
        #[starlark(require = named)] exclude_tags: Option<UnpackList<String>>,
        #[starlark(require = named)] include_paths: Option<UnpackList<String>>,
        #[starlark(require = named)] exclude_paths: Option<UnpackList<String>>,
    ) -> starlark::Result<WhenVal> {
        Ok(WhenVal {
            inner: WhenConfig::Push {
                include_branches: include_branches.map(|l| l.items),
                exclude_branches: exclude_branches.map(|l| l.items),
                include_tags: include_tags.map(|l| l.items),
                exclude_tags: exclude_tags.map(|l| l.items),
                include_paths: include_paths.map(|l| l.items),
                exclude_paths: exclude_paths.map(|l| l.items),
            },
        })
    }

    /// A Starlark function to construct a PullRequest object.
    /// Returns a `Rule` Starlark value that can be used in a Pipeline's `when` list.
    fn PullRequest<'v>(
        #[starlark(require = named)] include_branches: Option<UnpackList<String>>,
        #[starlark(require = named)] exclude_branches: Option<UnpackList<String>>,
        #[starlark(require = named)] include_paths: Option<UnpackList<String>>,
        #[starlark(require = named)] exclude_paths: Option<UnpackList<String>>,
    ) -> starlark::Result<WhenVal> {
        Ok(WhenVal {
            inner: WhenConfig::PullRequest {
                include_branches: include_branches.map(|l| l.items),
                exclude_branches: exclude_branches.map(|l| l.items),
                include_paths: include_paths.map(|l| l.items),
                exclude_paths: exclude_paths.map(|l| l.items),
            },
        })
    }

    /// A Starlark function to construct a Pipeline object.
    /// Returns None — pipelines are tracked in the Collector, not returned.
    fn Pipeline<'v>(
        #[starlark(require = named)] name: &str,
        #[starlark(require = named)]
        #[starlark(default = NoneType)]
        when: Value<'v>,
        #[starlark(require = named)]
        #[starlark(default = NoneType)]
        needs: Value<'v>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> starlark::Result<NoneType> {
        let needs = {
            let mut needs_set = HashSet::new();
            if !needs.is_none() {
                for item in needs.iterate(eval.heap())? {
                    if let Some(job) = JobVal::from_value(item) {
                        needs_set.insert(NeedConfig {
                            job_id: job.inner.job_id,
                            state: JobState::Completed,
                        });
                    } else if let Some(need) = NeedVal::from_value(item) {
                        needs_set.insert(need.inner);
                    } else {
                        return Err(starlark::Error::new_other(anyhow::anyhow!(
                            "Pipeline.needs: expected Need or Job, got {}",
                            item.get_type()
                        )));
                    }
                }
            }
            needs_set.into_iter().collect::<Vec<NeedConfig>>()
        };

        let when = {
            let mut when_set = HashSet::new();
            if !when.is_none() {
                for item in when.iterate(eval.heap())? {
                    let r = WhenVal::from_value(item).ok_or_else(|| {
                        starlark::Error::new_other(anyhow::anyhow!(
                            "Pipeline.when: expected When, got {}",
                            item.get_type()
                        ))
                    })?;
                    when_set.insert(r.inner.clone());
                }
            }
            when_set.into_iter().collect::<Vec<WhenConfig>>()
        };

        let pipeline_id = PipelineId::now_v7();

        let name = if name.is_empty() {
            format!("pipeline-{}", pipeline_id)
        } else {
            name.to_owned()
        };

        let pipeline = PipelineConfig {
            pipeline_id,
            name,
            when,
            needs,
        };

        eval.extra
            .ok_or_else(|| {
                starlark::Error::new_other(anyhow::anyhow!("no Collector in eval.extra"))
            })?
            .downcast_ref::<Collector>()
            .ok_or_else(|| {
                starlark::Error::new_other(anyhow::anyhow!("eval.extra is not a Collector"))
            })?
            .collect_pipeline(&pipeline);

        Ok(NoneType)
    }
}
