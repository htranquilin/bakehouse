//! Container runtime abstraction. v1 ships `AppleContainerRuntime` (bundled
//! apple/container CLI); the trait exists so a Colima/remote backend can be
//! swapped in without touching instance management (see spike/findings.md).

pub mod apple;
pub mod process;
pub mod sweeper;

use crate::error::Result;
use async_trait::async_trait;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::Path;
use tokio::sync::mpsc;

/// Label applied to every container Bakehouse creates; the sweeper enumerates by it.
pub const MANAGED_LABEL: &str = "com.bakehouse.managed";

#[derive(Debug, Clone)]
pub struct ContainerSpec {
    /// Container name (we use the instance id).
    pub name: String,
    /// Full image reference, digest-pinned.
    pub image: String,
    pub memory_mb: u32,
    pub env: BTreeMap<String, String>,
    /// name -> mount point
    pub volumes: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum ContainerState {
    Running { ip: String },
    Stopped,
    NotFound,
}

#[derive(Debug, Clone)]
pub struct ManagedContainer {
    pub name: String,
    pub state: ContainerState,
}

#[derive(Debug)]
pub struct ExecOutput {
    pub stdout: Vec<u8>,
    pub stderr: String,
    pub success: bool,
}

/// One raw progress line from an image pull.
pub type PullProgress = String;

#[async_trait]
pub trait ContainerRuntime: Send + Sync {
    /// Start the runtime services if not already running. Errors if a foreign
    /// (non-Bakehouse) installation of the same runtime is active.
    async fn ensure_ready(&self) -> Result<()>;
    /// Stop the runtime services (and any containers). No-op if not ours/not running.
    async fn shutdown(&self) -> Result<()>;

    async fn image_present(&self, image: &str) -> Result<bool>;
    async fn pull_image(&self, image: &str, progress: mpsc::Sender<PullProgress>) -> Result<()>;

    async fn run_container(&self, spec: &ContainerSpec) -> Result<()>;
    /// Run a one-shot command in a fresh container (e.g. root chown of a volume);
    /// the container is removed afterwards.
    async fn run_oneshot(
        &self,
        image: &str,
        user: &str,
        volumes: &BTreeMap<String, String>,
        cmd: &[&str],
    ) -> Result<()>;
    async fn stop_container(&self, name: &str) -> Result<()>;
    /// Remove a stopped container (containers are NOT started with --rm so that
    /// crash logs survive; callers must remove after reading logs).
    async fn remove_container(&self, name: &str) -> Result<()>;
    async fn container_state(&self, name: &str) -> Result<ContainerState>;
    async fn list_managed(&self) -> Result<Vec<ManagedContainer>>;
    async fn logs(&self, name: &str, tail: usize) -> Result<String>;

    async fn exec(&self, name: &str, cmd: &[&str], as_root: bool) -> Result<ExecOutput>;
    async fn copy_in(&self, name: &str, host: &Path, dest: &str) -> Result<()>;
    async fn copy_out(&self, name: &str, src: &str, host: &Path) -> Result<()>;

    async fn create_volume(&self, name: &str) -> Result<()>;
    async fn delete_volume(&self, name: &str) -> Result<()>;
}
