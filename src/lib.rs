use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::path::Path;

use allocative::Allocative;
use anyhow::Context as _;
use serde::{Deserialize, Serialize};
use starlark::any::ProvidesStaticType;
use starlark::environment::{FrozenModule, Globals, GlobalsBuilder, Module};
use starlark::eval::{Evaluator, ReturnFileLoader};
use starlark::syntax::{AstModule, Dialect, DialectTypes};
use starlark::values::list::{ListRef, UnpackList};
use starlark::values::{FrozenHeapName, NoSerialize, StarlarkValue, Value, none::NoneType};
use starlark::{starlark_module, starlark_simple_value};
use starlark_derive::starlark_value;
use uuid::Uuid;

// ── Pure Rust data records ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretRecord {
    pub name: String,
    pub key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepRecord {
    pub name: String,
    pub command: String,
    pub secrets: Vec<SecretRecord>,
}

/// A trigger rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RuleRecord {
    Push {
        include_branches: Option<Vec<String>>,
        exclude_branches: Option<Vec<String>>,
        include_tags: Option<Vec<String>>,
        exclude_tags: Option<Vec<String>>,
        include_paths: Option<Vec<String>>,
        exclude_paths: Option<Vec<String>>,
    },
    PullRequest {
        include_branches: Option<Vec<String>>,
        exclude_branches: Option<Vec<String>>,
        include_paths: Option<Vec<String>>,
        exclude_paths: Option<Vec<String>>,
    },
}

pub type JobRecordId = Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobRecord {
    pub job_id: JobRecordId,
    pub name: String,
    pub steps: Vec<StepRecord>,
    pub depends_on: Vec<JobRecordId>,
}

pub type PipelineId = Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineRecord {
    pub pipeline_id: PipelineId,
    pub name: String,
    pub when: Vec<RuleRecord>,
    pub targets: Vec<JobRecordId>,
}

// ── Collector (stored in eval.extra) ─────────────────────────────────────────

#[derive(Debug, Default, ProvidesStaticType)]
struct Collector {
    jobs: RefCell<Vec<JobRecord>>,
    pipelines: RefCell<Vec<PipelineRecord>>,
}

// ── Starlark value wrappers ───────────────────────────────────────────────────

#[derive(Debug, ProvidesStaticType, NoSerialize, Allocative)]
pub struct SecretVal {
    #[allocative(skip)]
    pub inner: SecretRecord,
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

// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, ProvidesStaticType, NoSerialize, Allocative)]
pub struct StepVal {
    #[allocative(skip)]
    pub inner: StepRecord,
}

impl fmt::Display for StepVal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Step(name={:?})", self.inner.name)
    }
}

starlark_simple_value!(StepVal);

#[starlark_value(type = "Step")]
impl<'v> StarlarkValue<'v> for StepVal {}

// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, ProvidesStaticType, NoSerialize, Allocative)]
pub struct JobVal {
    #[allocative(skip)]
    pub inner: JobRecord,
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
impl<'v> StarlarkValue<'v> for JobVal {}

// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, ProvidesStaticType, NoSerialize, Allocative)]
pub struct RuleVal {
    #[allocative(skip)]
    pub inner: RuleRecord,
}

impl fmt::Display for RuleVal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Rule({:?})", self.inner)
    }
}

starlark_simple_value!(RuleVal);

#[starlark_value(type = "Rule")]
impl<'v> StarlarkValue<'v> for RuleVal {}

// ── UUID v7 helper ────────────────────────────────────────────────────────────

#[inline]
fn new_uuid_v7() -> Uuid {
    Uuid::now_v7()
}

// ── Starlark globals module ───────────────────────────────────────────────────

/// Convert an anyhow-style error message into a starlark::Error.
#[inline]
fn serr(e: impl Into<anyhow::Error>) -> starlark::Error {
    starlark::Error::new_other(e)
}

#[starlark_module]
fn muzanci_globals(builder: &mut GlobalsBuilder) {
    fn Secret(name: &str, key: &str) -> starlark::Result<SecretVal> {
        Ok(SecretVal {
            inner: SecretRecord {
                name: name.to_owned(),
                key: key.to_owned(),
            },
        })
    }

    fn Step<'v>(
        command: &str,
        #[starlark(default = "")] name: &str,
        #[starlark(default = NoneType)] secrets: Value<'v>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> starlark::Result<StepVal> {
        let mut secrets_vec: Vec<SecretRecord> = Vec::new();
        if !secrets.is_none() {
            for item in secrets.iterate(eval.heap())? {
                let s = SecretVal::from_value(item).ok_or_else(|| {
                    serr(anyhow::anyhow!(
                        "Step.secrets: expected Secret, got {}",
                        item.get_type()
                    ))
                })?;
                secrets_vec.push(s.inner.clone());
            }
        }
        let name = if name.is_empty() { command } else { name };
        Ok(StepVal {
            inner: StepRecord {
                name: name.to_owned(),
                command: command.to_owned(),
                secrets: secrets_vec,
            },
        })
    }

    fn Job<'v>(
        steps: Value<'v>,
        #[starlark(default = "")] name: &str,
        #[starlark(default = NoneType)] depends_on: Value<'v>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> starlark::Result<JobVal> {
        let mut steps_vec: Vec<StepRecord> = Vec::new();
        for item in steps.iterate(eval.heap())? {
            let s = StepVal::from_value(item).ok_or_else(|| {
                serr(anyhow::anyhow!(
                    "Job.steps: expected Step, got {}",
                    item.get_type()
                ))
            })?;
            steps_vec.push(s.inner.clone());
        }

        let mut deps: Vec<JobRecordId> = Vec::new();
        if !depends_on.is_none() {
            for item in depends_on.iterate(eval.heap())? {
                let job = JobVal::from_value(item).ok_or_else(|| {
                    serr(anyhow::anyhow!(
                        "Job.depends_on: expected Job, got {}",
                        item.get_type()
                    ))
                })?;
                deps.push(job.inner.job_id);
            }
        }

        let job_id = uuid::Uuid::now_v7();

        let name = if name.is_empty() {
            format!("job-{}", job_id)
        } else {
            name.to_owned()
        };

        let record = JobRecord {
            job_id,
            name,
            steps: steps_vec,
            depends_on: deps,
        };

        let collector = eval
            .extra
            .ok_or_else(|| serr(anyhow::anyhow!("no Collector in eval.extra")))?
            .downcast_ref::<Collector>()
            .ok_or_else(|| serr(anyhow::anyhow!("eval.extra is not a Collector")))?;
        collector.jobs.borrow_mut().push(record.clone());

        Ok(JobVal { inner: record })
    }

    fn Push<'v>(
        include_branches: Option<UnpackList<String>>,
        exclude_branches: Option<UnpackList<String>>,
        include_tags: Option<UnpackList<String>>,
        exclude_tags: Option<UnpackList<String>>,
        include_paths: Option<UnpackList<String>>,
        exclude_paths: Option<UnpackList<String>>,
    ) -> starlark::Result<RuleVal> {
        Ok(RuleVal {
            inner: RuleRecord::Push {
                include_branches: include_branches.map(|l| l.items),
                exclude_branches: exclude_branches.map(|l| l.items),
                include_tags: include_tags.map(|l| l.items),
                exclude_tags: exclude_tags.map(|l| l.items),
                include_paths: include_paths.map(|l| l.items),
                exclude_paths: exclude_paths.map(|l| l.items),
            },
        })
    }

    fn PullRequest<'v>(
        include_branches: Option<UnpackList<String>>,
        exclude_branches: Option<UnpackList<String>>,
        include_paths: Option<UnpackList<String>>,
        exclude_paths: Option<UnpackList<String>>,
    ) -> starlark::Result<RuleVal> {
        Ok(RuleVal {
            inner: RuleRecord::PullRequest {
                include_branches: include_branches.map(|l| l.items),
                exclude_branches: exclude_branches.map(|l| l.items),
                include_paths: include_paths.map(|l| l.items),
                exclude_paths: exclude_paths.map(|l| l.items),
            },
        })
    }

    /// Define a pipeline.
    ///
    /// `predicates` must be a list of zero-argument lambdas returning bool.
    /// Example: `[lambda: GIT_BRANCH == "main"]`
    ///
    /// Each lambda is called immediately; both its repr() (pre-eval) and the
    /// resulting bool (post-eval) are stored in PredicateRecord.
    ///
    /// `depends_on` is an optional list of job values; only their UUIDs are
    /// stored in PipelineRecord.
    ///
    /// Returns None — pipelines are tracked in the Collector, not returned.
    fn Pipeline<'v>(
        name: &str,
        #[starlark(default = NoneType)] when: Value<'v>,
        #[starlark(default = NoneType)] targets: Value<'v>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> starlark::Result<NoneType> {
        let mut targets_vec: Vec<JobRecordId> = Vec::new();
        if !targets.is_none() {
            for item in targets.iterate(eval.heap())? {
                let j = JobVal::from_value(item).ok_or_else(|| {
                    serr(anyhow::anyhow!(
                        "Pipeline.depends_on: expected Job, got {}",
                        item.get_type()
                    ))
                })?;
                targets_vec.push(j.inner.job_id);
            }
        }

        let mut rules_vec: Vec<RuleRecord> = Vec::new();
        if !when.is_none() {
            for item in when.iterate(eval.heap())? {
                let r = RuleVal::from_value(item).ok_or_else(|| {
                    serr(anyhow::anyhow!(
                        "Pipeline.when: expected Rule, got {}",
                        item.get_type()
                    ))
                })?;
                rules_vec.push(r.inner.clone());
            }
        }

        let pipeline_id = uuid::Uuid::now_v7();

        let name = if name.is_empty() {
            format!("pipeline-{}", pipeline_id)
        } else {
            name.to_owned()
        };

        let record = PipelineRecord {
            pipeline_id,
            name,
            when: rules_vec,
            targets: targets_vec,
        };

        let collector = eval
            .extra
            .ok_or_else(|| serr(anyhow::anyhow!("no Collector in eval.extra")))?
            .downcast_ref::<Collector>()
            .ok_or_else(|| serr(anyhow::anyhow!("eval.extra is not a Collector")))?;
        collector.pipelines.borrow_mut().push(record);

        Ok(NoneType)
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Context values injected as Starlark globals before evaluation.
#[derive(Debug, Clone)]
pub struct EvalContext {
    pub git_repo: String,
    pub git_branch: String,
    pub git_commit: String,
}

/// Output of evaluating a root muzan.star file.
#[derive(Debug, Clone)]
pub struct EvalResult {
    /// All jobs constructed during evaluation, in construction order.
    pub jobs: Vec<JobRecord>,
    /// All pipelines constructed during evaluation, in construction order.
    pub pipelines: Vec<PipelineRecord>,
}

/// Build the Starlark Globals that include all MuzanCI builtins.
pub fn make_globals() -> Globals {
    GlobalsBuilder::standard().with(muzanci_globals).build()
}

/// Evaluate one Starlark file, recursively resolving its load() statements.
///
/// Each loaded module is frozen before being made available to its importer.
/// Context globals (GIT_REPO, GIT_BRANCH, GIT_COMMIT) are injected into
/// every module so that loaded helper modules can also reference them.
fn eval_file(
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
        let frozen = eval_file(&dep_path, globals, ctx, collector)?;
        resolved.push((id.clone(), frozen));
    }

    let module_map: HashMap<&str, &FrozenModule> =
        resolved.iter().map(|(k, v)| (k.as_str(), v)).collect();
    let mut loader = ReturnFileLoader {
        modules: &module_map,
    };

    let result = Module::with_temp_heap(|module| -> starlark::Result<FrozenModule> {
        let heap = module.heap();
        module.set("GIT_REPO", heap.alloc(ctx.git_repo.as_str()));
        module.set("GIT_BRANCH", heap.alloc(ctx.git_branch.as_str()));
        module.set("GIT_COMMIT", heap.alloc(ctx.git_commit.as_str()));

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

/// Evaluate a root muzan.star file and return all collected jobs and pipelines.
pub fn evaluate(root: &Path, ctx: &EvalContext) -> anyhow::Result<EvalResult> {
    let globals = make_globals();
    let collector = Collector::default();

    eval_file(root, &globals, ctx, &collector)
        .with_context(|| format!("evaluating {}", root.display()))?;

    Ok(EvalResult {
        jobs: collector.jobs.into_inner(),
        pipelines: collector.pipelines.into_inner(),
    })
}
