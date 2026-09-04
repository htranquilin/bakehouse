//! Headless verification of the SQL layer against a live container (M3).
//! Run: cargo run --bin sqlsmoke -- <resources-dir> <scratch-dir>
//! Requires the image already pulled into the scratch app-root (run smoke first).

use bakehouse_lib::instances::keychain;
use bakehouse_lib::runtime::apple::AppleContainerRuntime;
use bakehouse_lib::runtime::{ContainerRuntime, ContainerSpec, ContainerState};
use bakehouse_lib::sql::batch::split_batches;
use bakehouse_lib::sql::conn::{SqlConn, StreamEvent};
use std::collections::BTreeMap;
use std::time::Duration;

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let install_root: std::path::PathBuf = args.next().expect("usage").into();
    let scratch: std::path::PathBuf = args.next().expect("usage").into();

    let rt = AppleContainerRuntime::new(
        install_root.join("container-runtime"),
        scratch.join("app-root"),
        scratch.join("logs"),
    );
    rt.ensure_ready().await.expect("ensure_ready");

    let name = "bh-sqlsmoke";
    let volume = "bh-sqlsmoke-data";
    let _ = rt.stop_container(name).await;
    let _ = rt.remove_container(name).await;
    let _ = rt.delete_volume(volume).await;
    rt.create_volume(volume).await.expect("volume");

    let image = bakehouse_lib::instances::model::DEFAULT_IMAGE;
    let mut prep = BTreeMap::new();
    prep.insert(volume.to_string(), "/var/opt/mssql".to_string());
    rt.run_oneshot(image, "root", &prep, &["sh", "-c", "chown 10001:0 /var/opt/mssql && chmod 770 /var/opt/mssql"])
        .await
        .expect("prep");

    let password = keychain::generate_password();
    let mut env = BTreeMap::new();
    env.insert("ACCEPT_EULA".into(), "Y".into());
    env.insert("MSSQL_SA_PASSWORD".into(), password.clone());
    env.insert("MSSQL_MEMORY_LIMIT_MB".into(), "3072".into());
    let spec = ContainerSpec {
        name: name.into(),
        image: image.into(),
        memory_mb: 4096,
        env,
        volumes: prep.clone(),
    };
    rt.run_container(&spec).await.expect("run");

    let mut ip = String::new();
    for _ in 0..60 {
        tokio::time::sleep(Duration::from_secs(2)).await;
        if rt.logs(name, 200).await.unwrap_or_default().contains("ready for client connections") {
            if let ContainerState::Running { ip: got } = rt.container_state(name).await.unwrap() {
                ip = got;
            }
            break;
        }
    }
    assert!(!ip.is_empty(), "SQL never ready");
    println!("SQL up at {ip}");

    let mut conn = SqlConn::connect(&ip, &password, None).await.expect("connect");
    println!("connected, spid {}", conn.spid());

    // 1. PRINT / info tokens (the vendored-fork feature)
    let mut infos = vec![];
    conn.execute_streaming("PRINT 'hello from bakehouse'", |e| {
        if let StreamEvent::Info { message, .. } = e {
            infos.push(message);
        }
    })
    .await
    .expect("print");
    assert_eq!(infos, vec!["hello from bakehouse"], "PRINT not surfaced");
    println!("1. PRINT surfaced OK");

    // 2. rows affected
    let mut affected = vec![];
    conn.execute_streaming(
        "CREATE TABLE ##bh (id INT); INSERT ##bh VALUES (1),(2),(3); UPDATE ##bh SET id = id + 1;",
        |e| {
            if let StreamEvent::RowsAffected(n) = e {
                affected.push(n);
            }
        },
    )
    .await
    .expect("dml");
    assert_eq!(affected, vec![3, 3], "rows affected wrong: {affected:?}");
    println!("2. rows affected OK: {affected:?}");

    // 3. result sets + rows
    let mut sets = 0;
    let mut rows = 0;
    conn.execute_streaming("SELECT id FROM ##bh ORDER BY id; SELECT COUNT(*) AS n FROM ##bh", |e| {
        match e {
            StreamEvent::ResultSet(_) => sets += 1,
            StreamEvent::Row(_) => rows += 1,
            _ => {}
        }
    })
    .await
    .expect("select");
    assert_eq!((sets, rows), (2, 4), "sets/rows: {sets}/{rows}");
    println!("3. multiple result sets OK");

    // 4. server error surfaces with line number, connection survives
    let mut errors = vec![];
    conn.execute_streaming("SELECT 1\nSELECT * FROM does_not_exist_xyz", |e| {
        if let StreamEvent::ServerError { number, line, .. } = e {
            errors.push((number, line));
        }
    })
    .await
    .expect("error batch should not be a transport error");
    assert_eq!(errors, vec![(208, 2)], "expected invalid-object 208 line 2: {errors:?}");
    let survived = conn.query_rows("SELECT 42").await.expect("survive");
    assert_eq!(survived[0][0].display, "42");
    println!("4. server error (208, line 2) surfaced; connection survived");

    // 5. GO batch splitting + CREATE VIEW workflow
    let script = "CREATE DATABASE bhtest\nGO\nUSE bhtest\nGO\nCREATE VIEW v_test AS SELECT 7 AS seven\nGO\nSELECT seven FROM v_test";
    let batches = split_batches(script);
    assert_eq!(batches.len(), 4);
    let mut last_row = String::new();
    for b in &batches {
        conn.execute_streaming(&b.sql, |e| {
            if let StreamEvent::Row(cells) = e {
                last_row = cells[0].display.clone();
            }
        })
        .await
        .expect("batch");
    }
    assert_eq!(last_row, "7", "view round-trip failed");
    println!("5. GO-split CREATE VIEW workflow OK");

    // 6. trancount
    conn.exec_simple("BEGIN TRAN").await.unwrap();
    assert_eq!(conn.trancount().await.unwrap(), 1);
    conn.exec_simple("ROLLBACK").await.unwrap();
    assert_eq!(conn.trancount().await.unwrap(), 0);
    println!("6. trancount tracking OK");

    // 7. cancel via KILL from a second connection
    let victim_spid = conn.spid();
    let mut killer = SqlConn::connect(&ip, &password, None).await.expect("killer connect");
    let kill_task = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(2)).await;
        killer.exec_simple(&format!("KILL {victim_spid}")).await
    });
    let start = std::time::Instant::now();
    let mut kill_errors = vec![];
    let result = conn
        .execute_streaming("WAITFOR DELAY '00:01:00'", |e| {
            if let StreamEvent::ServerError { number, .. } = e {
                kill_errors.push(number);
            }
        })
        .await;
    let elapsed = start.elapsed();
    // KILL surfaces either as a server-error token (Ok path) or a dropped
    // connection (Err path) — both count, as long as the WAITFOR ended early.
    assert!(
        elapsed < Duration::from_secs(10),
        "kill took too long: {elapsed:?} (result: {result:?}, errors: {kill_errors:?})"
    );
    kill_task.await.unwrap().expect("kill exec");
    // The victim connection must be unusable now (executor detects this via the
    // trancount probe and reconnects the tab).
    let victim_after = conn.trancount().await;
    assert!(victim_after.is_err(), "victim connection should be dead after KILL");
    let mut fresh = SqlConn::connect(&ip, &password, None).await.expect("reconnect");
    assert_eq!(fresh.query_rows("SELECT 1").await.unwrap()[0][0].display, "1");
    println!("7. KILL-cancel OK ({elapsed:?}, errors {kill_errors:?}), dead-conn detected, reconnect OK");

    // teardown
    rt.stop_container(name).await.unwrap();
    rt.remove_container(name).await.unwrap();
    rt.delete_volume(volume).await.unwrap();
    rt.shutdown().await.unwrap();
    println!("SQLSMOKE OK");
}
