//! ContainerRuntime over the bundled apple/container CLI.
//! Verified behavior and JSON shapes: spike/findings.md.

use super::process::Cli;
use super::{
    ContainerRuntime, ContainerSpec, ContainerState, ExecOutput, ManagedContainer, PullProgress,
    MANAGED_LABEL,
};
use crate::error::{AppError, Result};
use async_trait::async_trait;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;

pub struct AppleContainerRuntime {
    cli: Cli,
    /// Directory containing bin/ and libexec/ (the bundled resource tree).
    install_root: PathBuf,
    app_root: PathBuf,
    log_root: PathBuf,
}

impl AppleContainerRuntime {
    pub fn new(install_root: PathBuf, app_root: PathBuf, log_root: PathBuf) -> Self {
        Self {
            cli: Cli { binary: install_root.join("bin/container") },
            install_root,
            app_root,
            log_root,
        }
    }

    /// Path of a currently running container-apiserver, if any.
    fn running_apiserver_path() -> Option<PathBuf> {
        let out = std::process::Command::new("/bin/ps")
            .args(["-axo", "comm"])
            .output()
            .ok()?;
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .find(|l| l.trim_end().ends_with("bin/container-apiserver"))
            .map(|l| PathBuf::from(l.trim()))
    }

    fn ours(&self, apiserver: &Path) -> bool {
        apiserver.starts_with(&self.install_root)
    }

    /// True if an apiserver from OUR bundle is currently running; false if none.
    /// (Used by the sweeper, which must not start the runtime just to check.)
    pub async fn ensure_ready_if_ours(&self) -> Result<bool> {
        match Self::running_apiserver_path() {
            Some(p) if self.ours(&p) => Ok(true),
            _ => Ok(false),
        }
    }

    async fn inspect(&self, name: &str) -> Result<Option<Value>> {
        // `container inspect` exits non-zero for unknown containers.
        match self.cli.run(&["inspect", name]).await {
            Ok(json) => {
                let v: Value = serde_json::from_str(&json)
                    .map_err(|e| AppError::Internal(format!("bad inspect JSON: {e}")))?;
                Ok(v.as_array().and_then(|a| a.first()).cloned())
            }
            Err(_) => Ok(None),
        }
    }
}

fn state_of(entry: &Value) -> ContainerState {
    let status = &entry["status"];
    match status["state"].as_str() {
        Some("running") => {
            let ip = status["networks"][0]["ipv4Address"]
                .as_str()
                .and_then(|cidr| cidr.split('/').next())
                .unwrap_or_default()
                .to_string();
            ContainerState::Running { ip }
        }
        Some(_) => ContainerState::Stopped,
        None => ContainerState::Stopped,
    }
}

#[async_trait]
impl ContainerRuntime for AppleContainerRuntime {
    async fn ensure_ready(&self) -> Result<()> {
        if let Some(apiserver) = Self::running_apiserver_path() {
            if self.ours(&apiserver) {
                return Ok(());
            }
            return Err(AppError::runtime(
                "Another `container` installation is already running on this Mac",
                Some(format!(
                    "Found a running apiserver at {}. Stop it with `container system stop` and retry.",
                    apiserver.display()
                )),
            ));
        }
        let app_root = self.app_root.to_string_lossy().into_owned();
        let install_root = self.install_root.to_string_lossy().into_owned();
        let log_root = self.log_root.to_string_lossy().into_owned();
        self.cli
            .run(&[
                "system", "start",
                "--app-root", &app_root,
                "--install-root", &install_root,
                "--log-root", &log_root,
                "--enable-kernel-install",
                "--timeout", "60",
            ])
            .await?;
        tracing::info!("container runtime started (app-root: {app_root})");
        Ok(())
    }

    async fn shutdown(&self) -> Result<()> {
        match Self::running_apiserver_path() {
            Some(apiserver) if self.ours(&apiserver) => {
                self.cli.run(&["system", "stop"]).await?;
                tracing::info!("container runtime stopped");
                Ok(())
            }
            _ => Ok(()), // not running, or not ours to stop
        }
    }

    async fn image_present(&self, image: &str) -> Result<bool> {
        let json = self.cli.run(&["image", "list", "--format", "json"]).await?;
        let v: Value = serde_json::from_str(&json)
            .map_err(|e| AppError::Internal(format!("bad image list JSON: {e}")))?;
        // The local store keeps the tag form under configuration.name (digest
        // stripped, and its descriptor digest is a locally-computed index digest)
        // — so match by the repo:tag part of our pinned reference.
        let want_tag = image.split('@').next().unwrap_or(image);
        Ok(v.as_array().is_some_and(|imgs| {
            imgs.iter().any(|i| {
                let name = i["configuration"]["name"].as_str().unwrap_or_default();
                name == image || name == want_tag
            })
        }))
    }

    async fn pull_image(&self, image: &str, progress: mpsc::Sender<PullProgress>) -> Result<()> {
        self.cli
            .run_streaming(
                &["image", "pull", "--platform", "linux/amd64", image],
                move |line| {
                    let _ = progress.try_send(line);
                },
            )
            .await
    }

    async fn run_container(&self, spec: &ContainerSpec) -> Result<()> {
        // Stale container with the same name (e.g. exited after a crash)? Clear it.
        if self.inspect(&spec.name).await?.is_some() {
            let _ = self.cli.run(&["delete", &spec.name]).await;
        }
        let mem = format!("{}M", spec.memory_mb);
        let label = format!("{MANAGED_LABEL}=true");
        // No --rm: an exited container must keep its logs readable for diagnostics;
        // stop/remove are explicit (see trait docs).
        let mut args: Vec<String> = vec![
            "run".into(), "-d".into(),
            "--rosetta".into(), "--arch".into(), "amd64".into(),
            "-m".into(), mem,
            "-l".into(), label,
            "--name".into(), spec.name.clone(),
        ];
        for (k, v) in &spec.env {
            args.push("-e".into());
            args.push(format!("{k}={v}"));
        }
        for (vol, mount) in &spec.volumes {
            args.push("-v".into());
            args.push(format!("{vol}:{mount}"));
        }
        args.push(spec.image.clone());
        let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
        self.cli.run(&arg_refs).await?;
        Ok(())
    }

    async fn run_oneshot(
        &self,
        image: &str,
        user: &str,
        volumes: &BTreeMap<String, String>,
        cmd: &[&str],
    ) -> Result<()> {
        let mut args: Vec<String> = vec![
            "run".into(), "--rm".into(),
            "--rosetta".into(), "--arch".into(), "amd64".into(),
            "--user".into(), user.into(),
        ];
        for (vol, mount) in volumes {
            args.push("-v".into());
            args.push(format!("{vol}:{mount}"));
        }
        args.push(image.into());
        args.extend(cmd.iter().map(|s| s.to_string()));
        let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
        self.cli.run(&arg_refs).await?;
        Ok(())
    }

    async fn stop_container(&self, name: &str) -> Result<()> {
        self.cli.run(&["stop", name]).await?;
        Ok(())
    }

    async fn remove_container(&self, name: &str) -> Result<()> {
        self.cli.run(&["delete", name]).await?;
        Ok(())
    }

    async fn container_state(&self, name: &str) -> Result<ContainerState> {
        match self.inspect(name).await? {
            Some(entry) => Ok(state_of(&entry)),
            None => Ok(ContainerState::NotFound),
        }
    }

    async fn list_managed(&self) -> Result<Vec<ManagedContainer>> {
        let json = self.cli.run(&["list", "--all", "--format", "json"]).await?;
        let v: Value = serde_json::from_str(&json)
            .map_err(|e| AppError::Internal(format!("bad list JSON: {e}")))?;
        let mut managed = vec![];
        if let Some(arr) = v.as_array() {
            for entry in arr {
                let labels = &entry["configuration"]["labels"];
                if labels[MANAGED_LABEL].as_str() == Some("true") {
                    managed.push(ManagedContainer {
                        name: entry["configuration"]["id"].as_str().unwrap_or_default().into(),
                        state: state_of(entry),
                    });
                }
            }
        }
        Ok(managed)
    }

    async fn logs(&self, name: &str, tail: usize) -> Result<String> {
        let out = self.cli.run(&["logs", name]).await?;
        let lines: Vec<&str> = out.lines().collect();
        let start = lines.len().saturating_sub(tail);
        Ok(lines[start..].join("\n"))
    }

    async fn exec(&self, name: &str, cmd: &[&str], as_root: bool) -> Result<ExecOutput> {
        let mut args = vec!["exec"];
        if as_root {
            args.extend(["--user", "root"]);
        }
        args.push(name);
        args.extend(cmd);
        match self.cli.run_raw(&args).await {
            Ok(stdout) => Ok(ExecOutput { stdout, stderr: String::new(), success: true }),
            Err(AppError::Runtime { message, detail }) => {
                Ok(ExecOutput { stdout: vec![], stderr: detail.unwrap_or(message), success: false })
            }
            Err(e) => Err(e),
        }
    }

    async fn copy_in(&self, name: &str, host: &Path, dest: &str) -> Result<()> {
        let file = tokio::fs::File::open(host)
            .await
            .map_err(|e| AppError::runtime(format!("cannot read {}: {e}", host.display()), None))?;
        let script = format!("cat > '{}'", dest.replace('\'', "'\\''"));
        self.cli
            .run_with_stdin(&["exec", "-i", name, "sh", "-c", &script], file)
            .await
    }

    async fn copy_out(&self, name: &str, src: &str, host: &Path) -> Result<()> {
        let bytes = self.cli.run_raw(&["exec", name, "cat", src]).await?;
        tokio::fs::write(host, bytes)
            .await
            .map_err(|e| AppError::runtime(format!("cannot write {}: {e}", host.display()), None))?;
        Ok(())
    }

    async fn create_volume(&self, name: &str) -> Result<()> {
        self.cli.run(&["volume", "create", name]).await?;
        Ok(())
    }

    async fn delete_volume(&self, name: &str) -> Result<()> {
        self.cli.run(&["volume", "delete", name]).await?;
        Ok(())
    }
}
