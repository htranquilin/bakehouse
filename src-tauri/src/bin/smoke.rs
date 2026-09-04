//! Headless smoke test for the container runtime layer (M2 verification).
//! Run with: cargo run --bin smoke -- <resources-dir> <scratch-dir>
//! Exercises: ensure_ready, volume create, run w/ volume, SQL readiness,
//! data persistence across container restart, stop, shutdown.

use bakehouse_lib::instances::keychain;
use bakehouse_lib::runtime::apple::AppleContainerRuntime;
use bakehouse_lib::runtime::{ContainerRuntime, ContainerSpec, ContainerState};
use std::collections::BTreeMap;
use std::time::Duration;

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let install_root: std::path::PathBuf =
        args.next().expect("usage: smoke <install-root> <scratch>").into();
    let scratch: std::path::PathBuf = args.next().expect("usage: smoke <install-root> <scratch>").into();
    std::fs::create_dir_all(scratch.join("app-root")).unwrap();
    std::fs::create_dir_all(scratch.join("logs")).unwrap();

    let rt = AppleContainerRuntime::new(
        install_root.join("container-runtime"),
        scratch.join("app-root"),
        scratch.join("logs"),
    );

    println!("1. ensure_ready");
    rt.ensure_ready().await.expect("ensure_ready");

    let name = "bh-smoke";
    let volume = "bh-smoke-data";
    println!("2. create volume");
    // Clean any leftovers from a previous (possibly crashed) run.
    let _ = rt.stop_container("bh-smoke").await;
    let _ = rt.remove_container("bh-smoke").await;
    let _ = rt.delete_volume(volume).await;
    rt.create_volume(volume).await.expect("create_volume");

    let password = keychain::generate_password();
    let image = bakehouse_lib::instances::model::DEFAULT_IMAGE;
    println!("3. image_present: {}", rt.image_present(image).await.expect("image_present"));

    println!("3b. prep volume ownership (uid 10001)");
    let mut prep = BTreeMap::new();
    prep.insert(volume.to_string(), "/var/opt/mssql".to_string());
    rt.run_oneshot(image, "root", &prep, &[
        "sh", "-c", "chown 10001:0 /var/opt/mssql && chmod 770 /var/opt/mssql",
    ])
    .await
    .expect("prep volume");

    let mut env = BTreeMap::new();
    env.insert("ACCEPT_EULA".into(), "Y".into());
    env.insert("MSSQL_SA_PASSWORD".into(), password.clone());
    env.insert("MSSQL_MEMORY_LIMIT_MB".into(), "3072".into());
    let mut volumes = BTreeMap::new();
    volumes.insert(volume.to_string(), "/var/opt/mssql".into());
    let spec = ContainerSpec {
        name: name.into(),
        image: image.into(),
        memory_mb: 4096,
        env,
        volumes,
    };

    println!("4. run container (with data volume)");
    rt.run_container(&spec).await.expect("run_container");

    println!("5. wait for SQL ready marker");
    let mut ip = String::new();
    for i in 0..60 {
        tokio::time::sleep(Duration::from_secs(2)).await;
        let logs = rt.logs(name, 200).await.unwrap_or_default();
        if logs.contains("ready for client connections") {
            if let ContainerState::Running { ip: got } = rt.container_state(name).await.unwrap() {
                ip = got;
            }
            println!("   ready after ~{}s at {ip}", (i + 1) * 2);
            break;
        }
    }
    assert!(!ip.is_empty(), "SQL never became ready");

    println!("6. TCP check {ip}:1433");
    tokio::net::TcpStream::connect((ip.as_str(), 1433)).await.expect("tcp connect");

    println!("7. write a marker file into the volume, then restart container");
    let out = rt
        .exec(name, &["sh", "-c", "echo bakehouse > /var/opt/mssql/bh-marker"], false)
        .await
        .expect("exec");
    assert!(out.success, "marker write failed: {}", out.stderr);
    rt.stop_container(name).await.expect("stop");
    assert_eq!(rt.container_state(name).await.unwrap(), ContainerState::Stopped, "stopped container should persist for logs");
    rt.remove_container(name).await.expect("remove");
    assert_eq!(rt.container_state(name).await.unwrap(), ContainerState::NotFound);

    rt.run_container(&spec).await.expect("re-run");
    tokio::time::sleep(Duration::from_secs(5)).await;
    let out = rt.exec(name, &["cat", "/var/opt/mssql/bh-marker"], false).await.expect("exec");
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("bakehouse"),
        "volume data did not survive restart"
    );
    println!("   volume persistence OK");

    println!("8. teardown");
    rt.stop_container(name).await.expect("stop 2");
    rt.remove_container(name).await.expect("remove 2");
    rt.delete_volume(volume).await.expect("delete_volume");
    rt.shutdown().await.expect("shutdown");

    println!("SMOKE OK");
}
