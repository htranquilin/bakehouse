//! Live connection manifest at ~/.bakehouse/connections.json, kept current on
//! every instance state change so external tools (CLI clients, Claude skills,
//! scripts) can discover and connect to running instances without asking the
//! app. Written with 0600 permissions — it contains sa passwords for the local
//! dev containers.

use super::keychain;
use super::model::{Instance, InstanceState};
use crate::error::Result;
use std::path::PathBuf;

pub fn connections_path() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".bakehouse/connections.json"))
}

pub fn write(entries: &[(Instance, InstanceState)]) -> Result<()> {
    let Some(path) = connections_path() else { return Ok(()) };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }

    let instances: Vec<serde_json::Value> = entries
        .iter()
        .map(|(inst, state)| {
            let mut v = serde_json::json!({
                "name": inst.name,
                "id": inst.id,
                "image": inst.image,
                "state": state.label(),
            });
            if let InstanceState::Running { ip } = state {
                if let Ok(password) = keychain::get(&inst.id) {
                    v["host"] = serde_json::json!(ip);
                    v["port"] = serde_json::json!(1433);
                    v["user"] = serde_json::json!("sa");
                    v["password"] = serde_json::json!(password);
                    v["connectionString"] = serde_json::json!(format!(
                        "Server={ip},1433;User Id=sa;Password={password};TrustServerCertificate=True"
                    ));
                    v["sqlcmd"] = serde_json::json!(format!(
                        "sqlcmd -S {ip},1433 -U sa -P '{password}' -C"
                    ));
                }
            }
            v
        })
        .collect();

    let doc = serde_json::json!({
        "$note": "Maintained by Bakehouse. Instances run only while the app is open; host IPs change on every instance start — always re-read this file before connecting. Connections require trusting the server's self-signed certificate (e.g. sqlcmd -C, TrustServerCertificate=True).",
        "updatedAt": chrono::Utc::now().to_rfc3339(),
        "instances": instances,
    });

    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(&doc).unwrap_or_default())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
    }
    std::fs::rename(&tmp, &path)?;
    Ok(())
}
