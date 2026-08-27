use std::path::Path;
use std::path::PathBuf;

use muzanci_git::GitCommitSha;
use muzanci_git::GitRemote;
use serde::Deserialize;
use serde::Serialize;

use muzanci_image::image::ImagePlatform;
use muzanci_image::manifest_ref::ManifestRef;

use crate::collector::Collector;
use crate::collector::Env;

/// An image to be used as a base for a job.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImageConfig {
    pub manifest_ref: ManifestRef,
    pub platform: ImagePlatform,
}

pub type StepId = uuid::Uuid;

/// A step to be executed in a job sandbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepConfig {
    pub step_id: StepId,
    pub name: String,
    pub command: String,
    pub image: Option<ImageConfig>,
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
pub enum JobStatus {
    Created,
    Queued,
    Started,
    Completed,
    Failed,
    TimedOut,
    CancelRequested,
    Cancelled,
    Skipped,
}

impl TryFrom<String> for JobStatus {
    type Error = anyhow::Error;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        match s.as_str() {
            "Completed" => Ok(JobStatus::Completed),
            "Failed" => Ok(JobStatus::Failed),
            _ => Err(anyhow::anyhow!("Invalid JobStatus string: {}", s)),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct NeedConfig {
    pub job_id: JobId,
    pub status: JobStatus,
}

/// A sequence of steps that execute in an isolated sandbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobConfig {
    pub steps: Vec<StepConfig>,
    pub name: String,
    pub image: ImageConfig,
    pub needs: Vec<NeedConfig>,
    pub job_id: JobId,
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

pub type PipelineId = uuid::Uuid;

/// A set of target jobs and a set of rules for when the pipeline should be created.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineConfig {
    pub name: String,
    pub when: Vec<WhenConfig>,
    pub needs: Vec<NeedConfig>,
    pub pipeline_id: PipelineId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerConfig {
    pub remote: GitRemote,
    pub commit_sha: GitCommitSha,
    pub input: PathBuf,
}

pub type DebugSessionId = uuid::Uuid;

pub type ServerId = uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugClientConfig {
    pub debug_session_id: DebugSessionId,
    pub server_id: ServerId,
}

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
