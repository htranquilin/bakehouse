//! Helpers for driving the bundled `container` CLI.

use crate::error::{AppError, Result};
use std::path::PathBuf;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

pub struct Cli {
    pub binary: PathBuf,
}

impl Cli {
    fn command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(&self.binary);
        cmd.args(args);
        cmd.kill_on_drop(true);
        cmd
    }

    /// Run to completion; error (with stderr detail) on non-zero exit.
    pub async fn run(&self, args: &[&str]) -> Result<String> {
        let out = self
            .command(args)
            .output()
            .await
            .map_err(|e| AppError::runtime(format!("failed to spawn container CLI: {e}"), None))?;
        if !out.status.success() {
            return Err(AppError::runtime(
                format!("`container {}` failed", args.join(" ")),
                Some(String::from_utf8_lossy(&out.stderr).into_owned()),
            ));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// Run and pass raw stdout bytes through (for copy-out streams).
    pub async fn run_raw(&self, args: &[&str]) -> Result<Vec<u8>> {
        let out = self
            .command(args)
            .output()
            .await
            .map_err(|e| AppError::runtime(format!("failed to spawn container CLI: {e}"), None))?;
        if !out.status.success() {
            return Err(AppError::runtime(
                format!("`container {}` failed", args.join(" ")),
                Some(String::from_utf8_lossy(&out.stderr).into_owned()),
            ));
        }
        Ok(out.stdout)
    }

    /// Run with `input` piped to stdin (for copy-in streams).
    pub async fn run_with_stdin(&self, args: &[&str], mut input: tokio::fs::File) -> Result<()> {
        let mut child = self
            .command(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| AppError::runtime(format!("failed to spawn container CLI: {e}"), None))?;
        let mut stdin = child.stdin.take().expect("piped stdin");
        tokio::io::copy(&mut input, &mut stdin)
            .await
            .map_err(|e| AppError::runtime(format!("stream into container failed: {e}"), None))?;
        drop(stdin);
        let out = child
            .wait_with_output()
            .await
            .map_err(|e| AppError::runtime(format!("container CLI wait failed: {e}"), None))?;
        if !out.status.success() {
            return Err(AppError::runtime(
                format!("`container {}` failed", args.join(" ")),
                Some(String::from_utf8_lossy(&out.stderr).into_owned()),
            ));
        }
        Ok(())
    }

    /// Run, forwarding each output line (stdout+stderr interleaved by line) to `on_line`.
    pub async fn run_streaming(
        &self,
        args: &[&str],
        on_line: impl Fn(String) + Send + Sync + 'static,
    ) -> Result<()> {
        let mut child = self
            .command(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| AppError::runtime(format!("failed to spawn container CLI: {e}"), None))?;

        let stdout = child.stdout.take().expect("piped stdout");
        let stderr = child.stderr.take().expect("piped stderr");
        let mut err_buf = String::new();

        let on_line = std::sync::Arc::new(on_line);
        let ol = on_line.clone();
        let out_task = tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                ol(line);
            }
        });
        let mut err_lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = err_lines.next_line().await {
            err_buf.push_str(&line);
            err_buf.push('\n');
            on_line(line);
        }
        let _ = out_task.await;

        let status = child
            .wait()
            .await
            .map_err(|e| AppError::runtime(format!("container CLI wait failed: {e}"), None))?;
        if !status.success() {
            return Err(AppError::runtime(
                format!("`container {}` failed", args.join(" ")),
                Some(err_buf),
            ));
        }
        Ok(())
    }
}
