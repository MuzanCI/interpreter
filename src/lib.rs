use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context as _;
use clap::Parser;
use serde::Deserialize;
use serde::Serialize;
use starlark::environment::GlobalsBuilder;
use url::Url;

use crate::collect::Collector;
use crate::collect::evaluate_file;
use crate::collect::predefined_primitives;
use crate::graph::walk_targets;

pub mod collect;
pub mod git;
pub mod graph;

/// MuzanCI pipeline interpreter.
///
/// Parses and evaluates a muzan.star configuration file, injecting the
/// provided CI context globals, and prints the resulting pipelines as JSON.
#[derive(Parser, Debug, Clone, Serialize, Deserialize)]
#[command(version, about)]
pub struct Args {
    /// Path to read the root pipeline configuration file.
    #[arg(default_value = "./external/customer-repo/muzan.py")]
    pub root_file: PathBuf,

    /// Git clone URL
    #[arg(long, default_value = "https://github.com/MuzanCI/customer-repo.git")]
    pub clone_url: Url,

    /// Git clone target dir
    #[arg(long, default_value = "./external/customer-repo")]
    pub clone_target_dir: PathBuf,

    /// Branch name
    #[arg(long, default_value = "main")]
    pub git_branch: String,

    /// Commit SHA
    #[arg(long, default_value = "62e23f12581dcd21d2fe57254aeed62b9afe1f54")]
    pub git_commit: String,

    #[arg(long, default_value = "./muzanci.eval_result.json")]
    /// Path to write the output JSON.
    pub output_file: PathBuf,
}

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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
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

/// A dependency from one job to another job's state.
#[derive(
    Debug,
    Clone,
    Copy,
    Serialize,
    Deserialize,
    PartialEq,
    Eq,
    Hash,
    strum::Display,
    strum::EnumString,
)]
pub enum JobState {
    Created,
    Ready,
    Started,
    Completed,
    Failed,
    Skipped,
}

impl TryFrom<String> for JobState {
    type Error = anyhow::Error;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        match s.as_str() {
            "Created" => Ok(JobState::Created),
            "Ready" => Ok(JobState::Ready),
            "Started" => Ok(JobState::Started),
            "Completed" => Ok(JobState::Completed),
            "Failed" => Ok(JobState::Failed),
            "Skipped" => Ok(JobState::Skipped),
            _ => Err(anyhow::anyhow!("Invalid JobState string: {}", s)),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct Need {
    pub job_id: JobId,
    pub state: JobState,
}

/// A sequence of steps that execute in an isolated sandbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub job_id: JobId,
    pub name: String,
    pub steps: Vec<Step>,
    pub needs: Vec<Need>,
}

pub type PipelineId = uuid::Uuid;

/// A set of target jobs and a set of rules for when the pipeline should be created.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pipeline {
    pub pipeline_id: PipelineId,
    pub name: String,
    pub when: Vec<Rule>,
    pub targets: Vec<Need>,
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Context values injected as Starlark globals before evaluation.
#[derive(Debug, Clone)]
pub struct EvalContext {
    pub git_branch: String,
    pub git_commit: String,
    pub git_clone_url: String,
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
        let (pipelines, job_registry) = {
            let globals = GlobalsBuilder::standard()
                .with(predefined_primitives)
                .build();
            let collector = Collector::default();

            evaluate_file(root, &globals, &self.eval_context, &collector)
                .with_context(|| format!("evaluating {}", root.display()))?;

            let job_registry = collector.job_registry.into_inner();
            let pipelines = collector.pipelines.into_inner();

            (pipelines, job_registry)
        };

        // Sanity check: assert that pipelines is consistent with job_registry.
        pipelines.iter().try_for_each(|pipeline| {
            pipeline.targets.iter().try_for_each(|need| {
                if !job_registry.contains_key(&need.job_id) {
                    anyhow::bail!(
                        "Pipeline [{}] has target need with job [{}] that is not defined",
                        pipeline.name,
                        need.job_id
                    );
                }
                Ok(())
            })?;
            Ok::<(), anyhow::Error>(())
        })?;

        // Prune jobs and job_graph to only include jobs reachable from the DAG
        // formed by the pipeline targets.
        let reachable_job_ids = {
            let mut reachable_job_ids = HashSet::new();

            pipelines.iter().try_for_each(|pipeline| {
                match walk_targets(pipeline.clone(), job_registry.clone()) {
                    Ok(job_ids) => {
                        reachable_job_ids.extend(job_ids);
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
