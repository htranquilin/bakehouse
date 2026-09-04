//! Instance registry + lifecycle state machine.
//! State transitions are pushed to the frontend via `instance:state` events.

use super::keychain;
use super::model::{Instance, InstanceState};
use super::store::InstanceStore;
use crate::error::{AppError, Result};
use crate::events;
use crate::runtime::{ContainerRuntime, ContainerSpec, ContainerState};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Duration;
use tauri::Emitter;
use tokio::sync::Mutex;

const SQL_READY_MARKER: &str = "ready for client connections";
const SQL_READY_TIMEOUT: Duration = Duration::from_secs(120);

pub struct InstanceManager {
    runtime: Arc<dyn ContainerRuntime>,
    store: InstanceStore,
    inner: Mutex<Inner>,
    app: tauri::AppHandle,
}

struct Inner {
    instances: Vec<Instance>,
    states: HashMap<String, InstanceState>,
}

#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct InstanceInfo {
    #[serde(flatten)]
    pub instance: Instance,
    pub state: InstanceState,
}

impl InstanceManager {
    pub fn new(
        runtime: Arc<dyn ContainerRuntime>,
        store: InstanceStore,
        app: tauri::AppHandle,
    ) -> Result<Self> {
        let instances = store.load()?;
        let states = instances
            .iter()
            .map(|i| (i.id.clone(), InstanceState::Stopped))
            .collect();
        Ok(Self { runtime, store, inner: Mutex::new(Inner { instances, states }), app })
    }

    fn emit_state(&self, instance_id: &str, state: &InstanceState) {
        let detail = match state {
            InstanceState::Failed { detail, .. } => Some(detail.clone()),
            InstanceState::Running { ip } => Some(ip.clone()),
            _ => None,
        };
        let _ = self.app.emit(
            events::INSTANCE_STATE,
            events::InstanceStatePayload {
                instance_id: instance_id.to_string(),
                state: state.label().to_string(),
                detail,
            },
        );
    }

    async fn set_state(&self, instance_id: &str, state: InstanceState) {
        let mut inner = self.inner.lock().await;
        inner.states.insert(instance_id.to_string(), state.clone());
        drop(inner);
        self.emit_state(instance_id, &state);
        self.write_connections_file().await;
    }

    /// Keep ~/.bakehouse/connections.json current for external tools.
    pub async fn write_connections_file(&self) {
        let entries: Vec<(Instance, InstanceState)> = {
            let inner = self.inner.lock().await;
            inner
                .instances
                .iter()
                .map(|i| {
                    (
                        i.clone(),
                        inner.states.get(&i.id).cloned().unwrap_or(InstanceState::Stopped),
                    )
                })
                .collect()
        };
        if let Err(e) = super::connections::write(&entries) {
            tracing::warn!("could not write connections.json: {e}");
        }
    }

    pub async fn list(&self) -> Vec<InstanceInfo> {
        let inner = self.inner.lock().await;
        inner
            .instances
            .iter()
            .map(|i| InstanceInfo {
                instance: i.clone(),
                state: inner.states.get(&i.id).cloned().unwrap_or(InstanceState::Stopped),
            })
            .collect()
    }

    pub async fn get(&self, instance_id: &str) -> Result<Instance> {
        let inner = self.inner.lock().await;
        inner
            .instances
            .iter()
            .find(|i| i.id == instance_id)
            .cloned()
            .ok_or_else(|| AppError::Internal(format!("unknown instance {instance_id}")))
    }

    pub async fn state(&self, instance_id: &str) -> InstanceState {
        let inner = self.inner.lock().await;
        inner.states.get(instance_id).cloned().unwrap_or(InstanceState::Stopped)
    }

    /// IP of a running instance (errors otherwise). SQL is always on port 1433 at this IP
    /// — published ports don't work for TDS (spike/findings.md).
    pub async fn running_ip(&self, instance_id: &str) -> Result<String> {
        match self.state(instance_id).await {
            InstanceState::Running { ip } => Ok(ip),
            other => Err(AppError::runtime(
                format!("instance is not running (state: {})", other.label()),
                None,
            )),
        }
    }

    pub async fn create(
        &self,
        name: String,
        memory_mb: u32,
        image: String,
    ) -> Result<InstanceInfo> {
        let instance = Instance::new(name, memory_mb, image);
        let password = keychain::generate_password();
        keychain::store(&instance.id, &password)?;

        let mut inner = self.inner.lock().await;
        inner.instances.push(instance.clone());
        inner.states.insert(instance.id.clone(), InstanceState::Stopped);
        self.store.save(&inner.instances)?;
        drop(inner);
        self.write_connections_file().await;

        Ok(InstanceInfo { instance, state: InstanceState::Stopped })
    }

    pub async fn update(
        &self,
        instance_id: &str,
        name: Option<String>,
        memory_mb: Option<u32>,
    ) -> Result<InstanceInfo> {
        let mut inner = self.inner.lock().await;
        let inst = inner
            .instances
            .iter_mut()
            .find(|i| i.id == instance_id)
            .ok_or_else(|| AppError::Internal(format!("unknown instance {instance_id}")))?;
        if let Some(n) = name {
            inst.name = n;
        }
        if let Some(m) = memory_mb {
            inst.memory_mb = m;
            inst.sql_memory_mb = inst.sql_memory_mb.min(m.saturating_sub(1024));
        }
        let info = InstanceInfo {
            instance: inst.clone(),
            state: inner.states.get(instance_id).cloned().unwrap_or(InstanceState::Stopped),
        };
        self.store.save(&inner.instances)?;
        drop(inner);
        self.write_connections_file().await;
        Ok(info)
    }

    /// Spawns the start sequence; progress arrives via events.
    pub async fn start(self: &Arc<Self>, instance_id: String) -> Result<()> {
        let instance = self.get(&instance_id).await?;
        match self.state(&instance_id).await {
            InstanceState::Stopped | InstanceState::Failed { .. } => {}
            s => {
                return Err(AppError::runtime(
                    format!("instance is already {}", s.label()),
                    None,
                ))
            }
        }
        let mgr = self.clone();
        tokio::spawn(async move {
            if let Err(e) = mgr.start_sequence(&instance).await {
                tracing::error!("instance {} failed to start: {e}", instance.id);
            }
        });
        Ok(())
    }

    async fn start_sequence(&self, instance: &Instance) -> Result<()> {
        let fail = |stage: &str, e: &AppError| InstanceState::Failed {
            stage: stage.into(),
            detail: e.to_string(),
        };

        self.set_state(&instance.id, InstanceState::Starting).await;
        if let Err(e) = self.runtime.ensure_ready().await {
            self.set_state(&instance.id, fail("runtime", &e)).await;
            return Err(e);
        }

        // Pull if the pinned image isn't local yet.
        match self.runtime.image_present(&instance.image).await {
            Ok(false) => {
                self.set_state(&instance.id, InstanceState::Pulling).await;
                let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(64);
                let app = self.app.clone();
                let job_id = instance.id.clone();
                tokio::spawn(async move {
                    while let Some(line) = rx.recv().await {
                        let _ = app.emit(
                            events::IMAGE_PULL_PROGRESS,
                            serde_json::json!({ "jobId": job_id, "line": line }),
                        );
                    }
                });
                if let Err(e) = self.runtime.pull_image(&instance.image, tx).await {
                    self.set_state(&instance.id, fail("pull", &e)).await;
                    return Err(e);
                }
                self.set_state(&instance.id, InstanceState::Starting).await;
            }
            Ok(true) => {}
            Err(e) => {
                self.set_state(&instance.id, fail("runtime", &e)).await;
                return Err(e);
            }
        }

        // Data volume (idempotent-ish: creation of an existing volume errors; ignore).
        let _ = self.runtime.create_volume(&instance.volume()).await;

        // Fresh volumes mount root-owned; sqlservr (uid 10001) dies on an
        // unwritable /var/opt/mssql. Cheap and idempotent, so run every start.
        let mut prep_volumes = BTreeMap::new();
        prep_volumes.insert(instance.volume(), "/var/opt/mssql".to_string());
        if let Err(e) = self
            .runtime
            .run_oneshot(
                &instance.image,
                "root",
                &prep_volumes,
                &["sh", "-c", "chown 10001:0 /var/opt/mssql && chmod 770 /var/opt/mssql"],
            )
            .await
        {
            self.set_state(&instance.id, fail("prepare-volume", &e)).await;
            return Err(e);
        }

        let password = keychain::get(&instance.id)?;
        let mut env = BTreeMap::new();
        env.insert("ACCEPT_EULA".into(), "Y".into());
        env.insert("MSSQL_SA_PASSWORD".into(), password);
        env.insert("MSSQL_MEMORY_LIMIT_MB".into(), instance.sql_memory_mb.to_string());
        let mut volumes = BTreeMap::new();
        volumes.insert(instance.volume(), "/var/opt/mssql".into());

        let spec = ContainerSpec {
            name: instance.id.clone(),
            image: instance.image.clone(),
            memory_mb: instance.memory_mb,
            env,
            volumes,
        };
        if let Err(e) = self.runtime.run_container(&spec).await {
            self.set_state(&instance.id, fail("start", &e)).await;
            return Err(e);
        }

        // Wait for SQL Server, watching container logs for the ready marker.
        self.set_state(&instance.id, InstanceState::WaitingForSql).await;
        let deadline = tokio::time::Instant::now() + SQL_READY_TIMEOUT;
        loop {
            tokio::time::sleep(Duration::from_secs(2)).await;
            let logs = self.runtime.logs(&instance.id, 200).await.unwrap_or_default();
            if logs.contains(SQL_READY_MARKER) {
                break;
            }
            match self.runtime.container_state(&instance.id).await {
                Ok(ContainerState::Running { .. }) => {}
                _ => {
                    let e = AppError::runtime(
                        "SQL Server exited during startup",
                        Some(logs.lines().rev().take(30).collect::<Vec<_>>().join("\n")),
                    );
                    self.set_state(&instance.id, fail("sql-startup", &e)).await;
                    return Err(e);
                }
            }
            if tokio::time::Instant::now() > deadline {
                let e = AppError::runtime(
                    "SQL Server did not become ready in time",
                    Some(logs.lines().rev().take(30).collect::<Vec<_>>().join("\n")),
                );
                self.set_state(&instance.id, fail("sql-timeout", &e)).await;
                return Err(e);
            }
        }

        match self.runtime.container_state(&instance.id).await {
            Ok(ContainerState::Running { ip }) => {
                tracing::info!("instance {} running at {ip}", instance.id);
                self.set_state(&instance.id, InstanceState::Running { ip }).await;
                Ok(())
            }
            Ok(_) | Err(_) => {
                let e = AppError::runtime("container state lost after SQL became ready", None);
                self.set_state(&instance.id, fail("start", &e)).await;
                Err(e)
            }
        }
    }

    pub async fn stop(&self, instance_id: &str) -> Result<()> {
        self.set_state(instance_id, InstanceState::Stopping).await;
        let result = self.runtime.stop_container(instance_id).await;
        // Containers run without --rm (crash logs must survive); remove explicitly.
        let _ = self.runtime.remove_container(instance_id).await;
        self.set_state(instance_id, InstanceState::Stopped).await;
        result
    }

    pub async fn delete(&self, instance_id: &str, delete_volume: bool) -> Result<()> {
        if let InstanceState::Running { .. } | InstanceState::WaitingForSql =
            self.state(instance_id).await
        {
            let _ = self.stop(instance_id).await;
        }
        let instance = self.get(instance_id).await?;
        // A stale stopped container (kept for logs) would hold the volume open.
        let _ = self.runtime.remove_container(instance_id).await;
        if delete_volume {
            let _ = self.runtime.delete_volume(&instance.volume()).await;
        }
        keychain::delete(instance_id);

        let mut inner = self.inner.lock().await;
        inner.instances.retain(|i| i.id != instance_id);
        inner.states.remove(instance_id);
        self.store.save(&inner.instances)?;
        drop(inner);
        self.write_connections_file().await;
        Ok(())
    }

    /// Stop everything that's running (app exit path).
    pub async fn stop_all(&self) {
        let ids: Vec<String> = {
            let inner = self.inner.lock().await;
            inner
                .states
                .iter()
                .filter(|(_, s)| {
                    matches!(
                        s,
                        InstanceState::Running { .. }
                            | InstanceState::WaitingForSql
                            | InstanceState::Starting
                    )
                })
                .map(|(id, _)| id.clone())
                .collect()
        };
        for id in ids {
            if let Err(e) = self.stop(&id).await {
                tracing::warn!("exit: failed to stop {id}: {e}");
            }
        }
    }
}
