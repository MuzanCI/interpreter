use std::path::Path;
use std::path::PathBuf;

use muzanci_git::GitCommitSha;
use muzanci_git::GitRemote;
use serde::Deserialize;
use serde::Serialize;

use muzanci_image::image::ImagePlatform;
use muzanci_image::manifest_ref::ManifestRef;
use uuid::Uuid;

use crate::collector::Collector;
use crate::collector::Env;

/// An image to be used as a base for a job.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImageConfig {
    pub manifest_ref: ManifestRef,
    pub platform: ImagePlatform,
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    sqlx::Type
)]
#[serde(transparent)]
#[sqlx(transparent)]
pub struct StepId(Uuid);

impl StepId {
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl std::fmt::Display for StepId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<Uuid> for StepId {
    fn from(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

impl From<StepId> for Uuid {
    fn from(id: StepId) -> Self {
        id.0
    }
}

impl TryFrom<&str> for StepId {
    type Error = uuid::Error;
    fn try_from(s: &str) -> Result<Self, Self::Error> {
        Ok(Self(Uuid::try_parse(s)?))
    }
}

impl std::ops::Deref for StepId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// A step to be executed in a job sandbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepConfig {
    pub step_id: StepId,
    pub name: String,
    pub command: String,
    pub image: Option<ImageConfig>,
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    sqlx::Type
)]
#[serde(transparent)]
#[sqlx(transparent)]
pub struct JobId(Uuid);

impl JobId {
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl std::fmt::Display for JobId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<Uuid> for JobId {
    fn from(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

impl From<JobId> for Uuid {
    fn from(id: JobId) -> Self {
        id.0
    }
}

impl TryFrom<&str> for JobId {
    type Error = uuid::Error;
    fn try_from(s: &str) -> Result<Self, Self::Error> {
        Ok(Self(Uuid::try_parse(s)?))
    }
}

impl std::ops::Deref for JobId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

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

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    sqlx::Type
)]
#[serde(transparent)]
#[sqlx(transparent)]
pub struct PipelineId(Uuid);

impl PipelineId {
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl std::fmt::Display for PipelineId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<Uuid> for PipelineId {
    fn from(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

impl From<PipelineId> for Uuid {
    fn from(id: PipelineId) -> Self {
        id.0
    }
}

impl TryFrom<&str> for PipelineId {
    type Error = uuid::Error;
    fn try_from(s: &str) -> Result<Self, Self::Error> {
        Ok(Self(Uuid::try_parse(s)?))
    }
}

impl std::ops::Deref for PipelineId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

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

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    sqlx::Type
)]
#[serde(transparent)]
#[sqlx(transparent)]
pub struct DebugSessionId(Uuid);

impl DebugSessionId {
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl std::fmt::Display for DebugSessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<Uuid> for DebugSessionId {
    fn from(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

impl From<DebugSessionId> for Uuid {
    fn from(id: DebugSessionId) -> Self {
        id.0
    }
}

impl TryFrom<&str> for DebugSessionId {
    type Error = uuid::Error;
    fn try_from(s: &str) -> Result<Self, Self::Error> {
        Ok(Self(Uuid::try_parse(s)?))
    }
}

impl std::ops::Deref for DebugSessionId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    sqlx::Type
)]
#[serde(transparent)]
#[sqlx(transparent)]
pub struct ServerId(Uuid);

impl ServerId {
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl std::fmt::Display for ServerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<Uuid> for ServerId {
    fn from(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

impl From<ServerId> for Uuid {
    fn from(id: ServerId) -> Self {
        id.0
    }
}

impl TryFrom<&str> for ServerId {
    type Error = uuid::Error;
    fn try_from(s: &str) -> Result<Self, Self::Error> {
        Ok(Self(Uuid::try_parse(s)?))
    }
}

impl std::ops::Deref for ServerId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

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
