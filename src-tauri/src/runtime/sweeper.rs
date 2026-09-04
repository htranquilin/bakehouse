//! Crash recovery: on launch, stop any Bakehouse-managed containers left over
//! from a previous crashed session, then leave the runtime stopped (it starts
//! lazily on first instance start).

use super::apple::AppleContainerRuntime;
use super::ContainerRuntime;

/// Best-effort; failures are logged, never fatal to app startup.
pub async fn sweep(runtime: &AppleContainerRuntime) {
    // Only act if an apiserver from OUR bundle is running (a crashed session
    // leaves it up, since it's launchd-managed).
    match runtime.ensure_ready_if_ours().await {
        Ok(false) => return, // nothing of ours running
        Ok(true) => {}
        Err(e) => {
            tracing::warn!("sweeper: cannot check runtime: {e}");
            return;
        }
    }
    match runtime.list_managed().await {
        Ok(managed) => {
            for c in &managed {
                tracing::info!("sweeper: stopping leftover container {}", c.name);
                if let Err(e) = runtime.stop_container(&c.name).await {
                    tracing::warn!("sweeper: failed to stop {}: {e}", c.name);
                }
                let _ = runtime.remove_container(&c.name).await;
            }
        }
        Err(e) => tracing::warn!("sweeper: list failed: {e}"),
    }
    if let Err(e) = runtime.shutdown().await {
        tracing::warn!("sweeper: shutdown failed: {e}");
    } else {
        tracing::info!("sweeper: leftover runtime stopped");
    }
}
