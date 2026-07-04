use std::collections::{HashMap, HashSet};
use std::path::Path;

use anyhow::Context as _;
use serde::{Deserialize, Serialize};
use starlark::environment::GlobalsBuilder;

pub mod collect;
pub mod graph;
use crate::collect::{Collector, evaluate_file, predefined_primitives};
use crate::graph::walk_targets;

/// A secret to be injected into a step's environment variables.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct Secret {
    pub name: String,
    pub key: String,
}

pub type StepId = uuid::Uuid;

/// A step to be executed in a job sandbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Step {
    pub step_id: StepId,
    pub name: String,
    pub command: String,
    pub secrets: Vec<Secret>,
}

/// A rule for when a pipeline should be created.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Rule {
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

pub type JobId = uuid::Uuid;

/// A sequence of steps that execute in an isolated sandbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub job_id: JobId,
    pub name: String,
    pub steps: Vec<Step>,
}

pub type PipelineId = uuid::Uuid;

/// A set of target jobs and a set of rules for when the pipeline should be created.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pipeline {
    pub pipeline_id: PipelineId,
    pub name: String,
    pub when: Vec<Rule>,
    pub targets: Vec<JobId>,
}

/// A mapping from a JobId to the list of JobIds that the job depends on.
/// Also known as a list adjacency list representation of a directed graph.
pub type JobDeps = HashMap<JobId, Vec<JobId>>;

// ── Public API ────────────────────────────────────────────────────────────────

/// Context values injected as Starlark globals before evaluation.
#[derive(Debug, Clone)]
pub struct EvalContext {
    pub git_repo: String,
    pub git_branch: String,
    pub git_commit: String,
}

/// Output of evaluating a root Starlark file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalResult {
    pub pipelines: Vec<Pipeline>,
    pub jobs: Vec<Job>,
}

pub struct Interpreter {
    eval_context: EvalContext,
}

impl Interpreter {
    /// Constructs a new Interpreter with the given evaluation context.
    pub fn new(eval_context: EvalContext) -> Self {
        Self { eval_context }
    }

    pub fn evaluate(&self, root: &Path) -> anyhow::Result<EvalResult> {
        // Evaluate the root file (including all `load`ed modules) and collect all pipeline, job definitions, and the job dependency graph.
        let (pipelines, job_registry, job_deps) = {
            let globals = GlobalsBuilder::standard()
                .with(predefined_primitives)
                .build();
            let collector = Collector::default();

            evaluate_file(root, &globals, &self.eval_context, &collector)
                .with_context(|| format!("evaluating {}", root.display()))?;

            let job_registry = collector.job_registry.into_inner();
            let job_deps = collector.job_deps.into_inner();
            let pipelines = collector.pipelines.into_inner();

            (pipelines, job_registry, job_deps)
        };

        // Sanity check: assert that pipelines is consistent with job_registry.
        pipelines.iter().try_for_each(|pipeline| {
            pipeline.targets.iter().try_for_each(|target_job_id| {
                if !job_registry.contains_key(target_job_id) {
                    anyhow::bail!(
                        "Pipeline {} has target job {} that is not defined",
                        pipeline.name,
                        target_job_id
                    );
                }
                Ok(())
            })?;
            Ok::<(), anyhow::Error>(())
        })?;

        // Sanity check: assert that job_registry is consistent with job_deps.
        job_deps.iter().try_for_each(|(job_id, deps)| {
            if !job_registry.contains_key(job_id) {
                anyhow::bail!("Job {} has dependencies but is not defined", job_id);
            }
            deps.iter().try_for_each(|dep| {
                if !job_registry.contains_key(dep) {
                    anyhow::bail!("Job {} has dependency {} that is not defined", job_id, dep);
                }
                Ok(())
            })?;
            Ok(())
        })?;

        // Prune jobs and job_deps to only include jobs reachable from the DAG
        // formed by the pipeline targets.
        let reachable_job_ids = {
            let mut reachable_job_ids = HashSet::new();

            pipelines.iter().try_for_each(|pipeline| {
                match walk_targets(pipeline.clone(), job_deps.clone()) {
                    Ok((jobs)) => {
                        jobs.iter().for_each(|job_id| {
                            reachable_job_ids.insert(job_id.clone());
                        });
                        Ok(())
                    }
                    Err(cycle) => {
                        anyhow::bail!(
                            "Pipeline {} has a cycle in its job dependencies: {:?}",
                            pipeline.name,
                            cycle
                        );
                    }
                }
            })?;

            reachable_job_ids
        };

        let jobs = job_registry
            .into_iter()
            .filter(|(job_id, _)| reachable_job_ids.contains(job_id))
            .map(|(_, job)| job)
            .collect::<Vec<Job>>();

        Ok(EvalResult { pipelines, jobs })
    }
}
