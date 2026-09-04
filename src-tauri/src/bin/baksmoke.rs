//! Headless verification of .bak inspect/restore + metadata queries (M4/M5).
//! Run: cargo run --bin baksmoke -- <resources-dir> <scratch-dir>

use bakehouse_lib::bak;
use bakehouse_lib::instances::keychain;
use bakehouse_lib::runtime::apple::AppleContainerRuntime;
use bakehouse_lib::runtime::{ContainerRuntime, ContainerSpec, ContainerState};
use bakehouse_lib::sql::conn::SqlConn;
use bakehouse_lib::sql::meta;
use bakehouse_lib::sql::session::SessionManager;
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

    let name = "bh-baksmoke";
    let volume = "bh-baksmoke-data";
    let _ = rt.stop_container(name).await;
    let _ = rt.remove_container(name).await;
    let _ = rt.delete_volume(volume).await;
    rt.create_volume(volume).await.expect("volume");

    let image = bakehouse_lib::instances::model::DEFAULT_IMAGE;
    let mut vols = BTreeMap::new();
    vols.insert(volume.to_string(), "/var/opt/mssql".to_string());
    rt.run_oneshot(image, "root", &vols, &["sh", "-c", "chown 10001:0 /var/opt/mssql && chmod 770 /var/opt/mssql"])
        .await
        .expect("prep");

    let password = keychain::generate_password();
    let mut env = BTreeMap::new();
    env.insert("ACCEPT_EULA".into(), "Y".into());
    env.insert("MSSQL_SA_PASSWORD".into(), password.clone());
    let spec = ContainerSpec {
        name: name.into(),
        image: image.into(),
        memory_mb: 4096,
        env,
        volumes: vols,
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
    assert!(!ip.is_empty());
    println!("SQL up at {ip}");

    // 1. Build a source db and back it up inside the container, then copy it out
    //    so we can exercise the real host->container staging path.
    let mut conn = SqlConn::connect(&ip, &password, None).await.expect("connect");
    conn.exec_simple("CREATE DATABASE vendor_src").await.unwrap();
    conn.exec_simple(
        "CREATE TABLE vendor_src.dbo.customers (id INT IDENTITY PRIMARY KEY, name NVARCHAR(80) NOT NULL, note NVARCHAR(200) NULL)",
    )
    .await
    .unwrap();
    conn.exec_simple("INSERT vendor_src.dbo.customers (name, note) VALUES (N'Ada', NULL), (N'Grace', N'x')")
        .await
        .unwrap();
    conn.exec_simple(
        "USE vendor_src; EXEC('CREATE VIEW dbo.v_customers AS SELECT id, name FROM dbo.customers'); USE master;",
    )
    .await
    .unwrap();
    conn.exec_simple("BACKUP DATABASE vendor_src TO DISK = N'/tmp/vendor_src.bak' WITH INIT, COMPRESSION")
        .await
        .unwrap();
    let host_bak = scratch.join("vendor_src.bak");
    rt.copy_out(name, "/tmp/vendor_src.bak", &host_bak).await.expect("copy_out");
    println!("1. source db + host .bak ready ({} bytes)", std::fs::metadata(&host_bak).unwrap().len());

    // 2. stage_and_inspect (the real M4 path). Inject an in-memory password
    //    source — unsigned test binaries must never touch the Keychain (each
    //    rebuild has a new code identity and triggers an auth dialog).
    let pw = password.clone();
    let sessions = SessionManager::new(std::sync::Arc::new(move |_id: &str| Ok(pw.clone())));
    let staged = bak::stage_and_inspect(&rt, &sessions, name, &ip, host_bak.to_str().unwrap())
        .await
        .expect("inspect");
    assert_eq!(staged.sets.len(), 1);
    let set = &staged.sets[0];
    assert_eq!(set.database_name, "vendor_src");
    assert_eq!(set.backup_type, "Full");
    assert!(!set.has_filestream);
    assert_eq!(set.files.len(), 2, "expected mdf+ldf: {:?}", set.files.iter().map(|f| &f.logical_name).collect::<Vec<_>>());
    println!("2. inspect OK: set #{} db={} files={:?}", set.file_number, set.database_name, set.files.iter().map(|f| f.suggested_target.clone()).collect::<Vec<_>>());

    // 3. Restore under a new name using the inspect output (same SQL shape as run_restore).
    let moves: Vec<String> = set
        .files
        .iter()
        .map(|f| format!("MOVE N'{}' TO N'{}'", f.logical_name, f.suggested_target.replace("vendor_src", "vendor_restored")))
        .collect();
    let restore_sql = format!(
        "RESTORE DATABASE [vendor_restored] FROM DISK = N'{}' WITH FILE = {}, {}, RECOVERY, STATS = 25",
        staged.staged_path, set.file_number, moves.join(", ")
    );
    conn.exec_simple(&restore_sql).await.expect("restore");
    let rows = conn
        .query_rows("SELECT COUNT(*) FROM vendor_restored.dbo.customers")
        .await
        .unwrap();
    assert_eq!(rows[0][0].display, "2");
    println!("3. restore + data verified OK");

    // 4. Metadata queries against the restored db.
    let tables = meta::objects(&mut conn, "vendor_restored", "table").await.unwrap();
    assert!(tables.iter().any(|t| t.name == "customers"));
    let views = meta::objects(&mut conn, "vendor_restored", "view").await.unwrap();
    assert_eq!(views.len(), 1);
    let cols = meta::columns(&mut conn, "vendor_restored", tables[0].object_id).await.unwrap();
    assert_eq!(cols.len(), 3);
    assert!(cols[0].is_pk && !cols[0].nullable);
    assert_eq!(cols[1].data_type, "nvarchar(80)");
    println!("4. meta objects/columns OK: {:?}", cols.iter().map(|c| format!("{} {}", c.name, c.data_type)).collect::<Vec<_>>());

    // 5. Script view as CREATE and ALTER; script table.
    let create_view = meta::script_object(&mut conn, "vendor_restored", views[0].object_id, false).await.unwrap();
    assert!(create_view.contains("CREATE VIEW"), "{create_view}");
    let alter_view = meta::script_object(&mut conn, "vendor_restored", views[0].object_id, true).await.unwrap();
    assert!(alter_view.contains("ALTER VIEW"), "{alter_view}");
    let create_table = meta::script_object(&mut conn, "vendor_restored", tables[0].object_id, false).await.unwrap();
    assert!(create_table.contains("CREATE TABLE") && create_table.contains("IDENTITY(1,1)") && create_table.contains("PRIMARY KEY"), "{create_table}");
    println!("5. script CREATE/ALTER OK");

    // 6. Completion schema shape.
    let schema = meta::completion_schema(&mut conn, "vendor_restored").await.unwrap();
    let obj = schema.as_object().unwrap();
    assert!(obj.contains_key("customers") && obj.contains_key("v_customers"), "{schema}");
    println!("6. completion schema OK");

    // 7. Generate-scripts: table + view together, dependency order, DROP IF EXISTS.
    let ids = vec![tables[0].object_id, views[0].object_id];
    let script = meta::generate_scripts(&mut conn, "vendor_restored", &ids, true, true).await.unwrap();
    let table_pos = script.find("CREATE TABLE").expect("table in script");
    let view_pos = script.find("CREATE VIEW").expect("view in script");
    assert!(table_pos < view_pos, "table must be scripted before the view");
    assert!(script.contains("DROP VIEW IF EXISTS [dbo].[v_customers]"), "{script}");
    assert!(script.contains("USE [vendor_restored]"));
    println!("7. generate_scripts OK ({} chars, table before view)", script.len());

    // teardown
    rt.stop_container(name).await.unwrap();
    rt.remove_container(name).await.unwrap();
    rt.delete_volume(volume).await.unwrap();
    rt.shutdown().await.unwrap();
    println!("BAKSMOKE OK");
}
