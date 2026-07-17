use std::path::Path;

use serde::Deserialize;
use serde::Serialize;

use crate::collector::Collector;
use crate::collector::Env;

/// A secret to be injected into a step's environment variables.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct SecretConfig {
    pub name: String,
    pub key: String,
}

pub type StepId = uuid::Uuid;

/// A step to be executed in a job sandbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepConfig {
    pub step_id: StepId,
    pub name: String,
    pub command: String,
    pub secrets: Vec<SecretConfig>,
}

/// A rule for when a pipeline should be created.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum WhenConfig {
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
    strum::EnumString
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
pub struct NeedConfig {
    pub job_id: JobId,
    pub state: JobState,
}

/// A sequence of steps that execute in an isolated sandbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobConfig {
    pub job_id: JobId,
    pub name: String,
    pub steps: Vec<StepConfig>,
    pub needs: Vec<NeedConfig>,
}

pub type PipelineId = uuid::Uuid;

/// A set of target jobs and a set of rules for when the pipeline should be created.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineConfig {
    pub pipeline_id: PipelineId,
    pub name: String,
    pub when: Vec<WhenConfig>,
    pub needs: Vec<NeedConfig>,
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Output of evaluating a root Starlark file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub pipelines: Vec<PipelineConfig>,
    pub jobs: Vec<JobConfig>,
}

impl Config {
    pub fn from_file(input: &Path, env: &Env) -> anyhow::Result<Self> {
        let collector = Collector::new(env);

        collector
            .evaluate(input)
            .map_err(|e| anyhow::anyhow!("failed to evaluate {}:\n{}", input.display(), e))?;

        collector
            .try_into()
            .map_err(|e| anyhow::anyhow!("failed to convert Collector into Config: {}", e))
    }

    pub fn to_ascii_graph(&self) -> String {
        unimplemented!();
    }

    pub fn to_json(&self) -> anyhow::Result<String> {
        serde_json::to_string_pretty(self)
            .map_err(|e| anyhow::anyhow!("failed to serialize Config to JSON: {}", e))
    }

    pub fn to_dot_graph(&self) -> String {
        unimplemented!();
    }
}
