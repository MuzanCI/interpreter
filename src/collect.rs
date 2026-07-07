use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::Path;

use allocative::Allocative;
use anyhow::Context as _;
use starlark::any::ProvidesStaticType;
use starlark::environment::{FrozenModule, Globals, GlobalsBuilder, Module};
use starlark::eval::{Evaluator, ReturnFileLoader};
use starlark::syntax::{AstModule, Dialect, DialectTypes};
use starlark::values::list::UnpackList;
use starlark::values::{FrozenHeapName, Heap, NoSerialize, StarlarkValue, Value, none::NoneType};
use starlark::{starlark_module, starlark_simple_value};
use starlark_derive::starlark_value;

use crate::{
    EvalContext, Job, JobId, JobState, Need, Pipeline, PipelineId, Rule, Secret, Step, StepId,
};

pub type JobRegistry = HashMap<JobId, Job>;

/// A collector of all jobs and pipelines constructed during evaluation.
/// This is used as the `extra` field of the Evaluator, so that the globals can
/// push jobs and pipelines into it. `extra` is an immutable reference, so we
/// use RefCell to allow interior mutability (borrowing is checked at runtime
/// instead of compile time).
#[derive(Debug, Default, ProvidesStaticType)]
pub struct Collector {
    pub job_registry: RefCell<JobRegistry>,
    pub pipelines: RefCell<Vec<Pipeline>>,
}

/// A Starlark value that wraps a [`Secret`].
#[derive(Debug, ProvidesStaticType, NoSerialize, Allocative)]
struct SecretVal {
    #[allocative(skip)]
    inner: Secret,
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
    inner: Step,
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
    inner: Job,
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
            inner: Need {
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
    inner: Need,
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
struct RuleVal {
    #[allocative(skip)]
    inner: Rule,
}

impl fmt::Display for RuleVal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Rule({:?})", self.inner)
    }
}

starlark_simple_value!(RuleVal);

#[starlark_value(type = "Rule")]
impl<'v> StarlarkValue<'v> for RuleVal {}

/// A Starlark module that provides predefined primitives for defining
/// pipeline configuration.
#[starlark_module]
pub fn predefined_primitives(builder: &mut GlobalsBuilder) {
    fn Secret(
        #[starlark(require = named)] name: &str,
        #[starlark(require = named)] key: &str,
    ) -> starlark::Result<SecretVal> {
        Ok(SecretVal {
            inner: Secret {
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
        let mut secrets_vec: Vec<Secret> = Vec::new();
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
            inner: Step {
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
        let collector = eval
            .extra
            .ok_or_else(|| {
                starlark::Error::new_other(anyhow::anyhow!("no Collector in eval.extra"))
            })?
            .downcast_ref::<Collector>()
            .ok_or_else(|| {
                starlark::Error::new_other(anyhow::anyhow!("eval.extra is not a Collector"))
            })?;

        let steps = {
            let mut steps_vec: Vec<Step> = Vec::new();
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
            let mut needs_set: HashSet<Need> = HashSet::new();
            if !needs.is_none() {
                for item in needs.iterate(eval.heap())? {
                    if let Some(job) = JobVal::from_value(item) {
                        needs_set.insert(Need {
                            job_id: job.inner.job_id,
                            state: JobState::Completed,
                        });
                    } else if let Some(need) = NeedVal::from_value(item) {
                        needs_set.insert(need.inner);
                    } else {
                        return Err(starlark::Error::new_other(anyhow::anyhow!(
                            "Job.needs: expected Need or Job, got {}",
                            item.get_type()
                        )));
                    }
                }
            }

            needs_set.into_iter().collect::<Vec<Need>>()
        };

        let job = {
            let job_id = JobId::now_v7();

            let name = if name.is_empty() {
                format!("job-{}", job_id)
            } else {
                name.to_owned()
            };

            Job {
                job_id,
                name,
                steps,
                needs,
            }
        };

        collector
            .job_registry
            .borrow_mut()
            .insert(job.job_id, job.clone());

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
    ) -> starlark::Result<RuleVal> {
        Ok(RuleVal {
            inner: Rule::Push {
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
    ) -> starlark::Result<RuleVal> {
        Ok(RuleVal {
            inner: Rule::PullRequest {
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
        targets: Value<'v>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> starlark::Result<NoneType> {
        let targets = {
            let mut targets_set = HashSet::new();
            if !targets.is_none() {
                for item in targets.iterate(eval.heap())? {
                    if let Some(job) = JobVal::from_value(item) {
                        targets_set.insert(Need {
                            job_id: job.inner.job_id,
                            state: JobState::Completed,
                        });
                    } else if let Some(need) = NeedVal::from_value(item) {
                        targets_set.insert(need.inner);
                    } else {
                        return Err(starlark::Error::new_other(anyhow::anyhow!(
                            "Job.needs: expected Need or Job, got {}",
                            item.get_type()
                        )));
                    }
                }
            }
            targets_set.into_iter().collect::<Vec<Need>>()
        };

        let when = {
            let mut rules_set = HashSet::new();
            if !when.is_none() {
                for item in when.iterate(eval.heap())? {
                    let r = RuleVal::from_value(item).ok_or_else(|| {
                        starlark::Error::new_other(anyhow::anyhow!(
                            "Pipeline.when: expected Rule, got {}",
                            item.get_type()
                        ))
                    })?;
                    rules_set.insert(r.inner.clone());
                }
            }
            rules_set.into_iter().collect::<Vec<Rule>>()
        };

        let pipeline_id = PipelineId::now_v7();

        let name = if name.is_empty() {
            format!("pipeline-{}", pipeline_id)
        } else {
            name.to_owned()
        };

        let pipeline = Pipeline {
            pipeline_id,
            name,
            when,
            targets,
        };

        let collector = eval
            .extra
            .ok_or_else(|| {
                starlark::Error::new_other(anyhow::anyhow!("no Collector in eval.extra"))
            })?
            .downcast_ref::<Collector>()
            .ok_or_else(|| {
                starlark::Error::new_other(anyhow::anyhow!("eval.extra is not a Collector"))
            })?;
        collector.pipelines.borrow_mut().push(pipeline.clone());

        Ok(NoneType)
    }
}

/// Evaluate one Starlark file, recursively resolving its load() statements.
/// Context globals (GIT_REPO, GIT_BRANCH, GIT_COMMIT) are injected into
/// every module so that loaded helper modules can also reference them.
/// Each loaded module is frozen before being made available to its importer.
pub fn evaluate_file(
    path: &Path,
    globals: &Globals,
    ctx: &EvalContext,
    collector: &Collector,
) -> anyhow::Result<FrozenModule> {
    let source =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;

    // Enable f-strings, type annotations, and top-level statements (for/if at
    // module level) used in pipeline config files.
    let dialect = Dialect {
        enable_f_strings: true,
        enable_types: DialectTypes::Enable,
        enable_top_level_stmt: true,
        ..Dialect::Standard
    };

    let ast = AstModule::parse(&path.to_string_lossy(), source, &dialect)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    // Collect all load() module IDs before the AST is consumed by eval_module.
    let load_ids: Vec<String> = ast
        .loads()
        .into_iter()
        .map(|l| l.module_id.to_owned())
        .collect();

    // Recursively resolve and freeze each loaded dependency.
    let base = path.parent().unwrap_or(Path::new("."));
    let mut resolved: Vec<(String, FrozenModule)> = Vec::new();
    for id in &load_ids {
        let dep_path = base.join(id.as_str());
        let frozen = evaluate_file(&dep_path, globals, ctx, collector)?;
        resolved.push((id.clone(), frozen));
    }

    let module_map: HashMap<&str, &FrozenModule> =
        resolved.iter().map(|(k, v)| (k.as_str(), v)).collect();
    let mut loader = ReturnFileLoader {
        modules: &module_map,
    };

    let result = Module::with_temp_heap(|module| -> starlark::Result<FrozenModule> {
        let heap = module.heap();
        module.set("GIT_BRANCH", heap.alloc(ctx.git_branch.as_str()));
        module.set("GIT_COMMIT", heap.alloc(ctx.git_commit.as_str()));
        module.set("GIT_CLONE_URL", heap.alloc(ctx.git_clone_url.as_str()));

        {
            let mut eval = Evaluator::new(&module);
            eval.set_loader(&mut loader);
            eval.extra = Some(collector);
            eval.eval_module(ast, globals)?;
        }

        Ok(module.freeze_named(FrozenHeapName::User(Box::new(
            path.to_string_lossy().into_owned(),
        )))?)
    });

    result.map_err(|e| anyhow::anyhow!("{}", e))
}
