pub mod bak;
mod commands;
pub mod csvimport;
pub mod error;
mod events;
mod history;
pub mod instances;
mod paths;
pub mod results;
pub mod runtime;
mod settings;
pub mod sql;

pub use error::{AppError, Result};

use instances::manager::InstanceManager;
use runtime::apple::AppleContainerRuntime;
use runtime::ContainerRuntime;
use std::sync::Arc;
use tauri::Manager;

pub struct AppState {
    pub paths: paths::AppPaths,
    pub settings: Arc<settings::SettingsStore>,
    pub runtime: Arc<AppleContainerRuntime>,
    pub instances: Arc<InstanceManager>,
    pub sessions: Arc<sql::session::SessionManager>,
    pub results: Arc<results::store::ResultStore>,
    pub history: Arc<history::HistoryStore>,
}

#[tauri::command]
fn app_info(state: tauri::State<AppState>) -> serde_json::Value {
    serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "appSupport": state.paths.app_support.display().to_string(),
    })
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .setup(|app| {
            let paths = paths::AppPaths::new(app.handle())?;
            paths.ensure_dirs()?;

            let file_appender = tracing_appender::rolling::daily(paths.logs(), "bakehouse.log");
            let (writer, guard) = tracing_appender::non_blocking(file_appender);
            app.manage(guard);
            tracing_subscriber::fmt()
                .with_writer(writer)
                .with_ansi(false)
                .with_env_filter(
                    tracing_subscriber::EnvFilter::try_from_default_env()
                        .unwrap_or_else(|_| "info".into()),
                )
                .init();
            tracing::info!(version = env!("CARGO_PKG_VERSION"), "bakehouse starting");

            let install_root = app
                .path()
                .resolve("resources/container-runtime", tauri::path::BaseDirectory::Resource)?;
            let runtime = Arc::new(AppleContainerRuntime::new(
                install_root,
                paths.container_root(),
                paths.container_logs(),
            ));

            let settings = Arc::new(settings::SettingsStore::load(paths.settings_file()));
            let store = instances::store::InstanceStore::new(paths.instances_file());
            let instances = Arc::new(InstanceManager::new(
                runtime.clone() as Arc<dyn ContainerRuntime>,
                store,
                app.handle().clone(),
            )?);

            // Crash recovery from a previous session, off the main thread; also
            // reset connections.json (a crash may have left stale "running" rows).
            let sweep_runtime = runtime.clone();
            let boot_instances = instances.clone();
            tauri::async_runtime::spawn(async move {
                runtime::sweeper::sweep(&sweep_runtime).await;
                boot_instances.write_connections_file().await;
            });

            let sessions = Arc::new(sql::session::SessionManager::with_keychain());
            let results = Arc::new(results::store::ResultStore::new());
            let history = Arc::new(history::HistoryStore::open(&paths.history_db())?);

            app.manage(AppState { paths, settings, runtime, instances, sessions, results, history });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_info,
            commands::setup::setup_status,
            commands::setup::setup_accept_eula,
            commands::setup::runtime_shutdown,
            commands::setup::diag_instance_logs,
            commands::setup::diag_app_log,
            commands::instances::instance_list,
            commands::instances::sql_versions,
            commands::instances::instance_create,
            commands::instances::instance_start,
            commands::instances::instance_stop,
            commands::instances::instance_update,
            commands::instances::instance_delete,
            commands::instances::instance_reveal_password,
            commands::instances::instance_export_compose,
            commands::sessions::session_open,
            commands::sessions::session_close,
            commands::sessions::session_set_database,
            commands::sessions::session_state,
            commands::sessions::db_list,
            commands::query::query_run,
            commands::query::query_run_uncapped,
            commands::query::query_cancel,
            commands::results::results_window,
            commands::results::results_cell,
            commands::results::results_sort,
            commands::results::results_export_csv,
            commands::results::results_copy_tsv,
            commands::results::results_release,
            commands::history::history_list,
            commands::history::buffers_save,
            commands::history::buffers_load_all,
            commands::history::buffers_delete,
            commands::history::file_read_sql,
            commands::history::file_write_sql,
            commands::csvimport::csv_scan_dir,
            commands::csvimport::csv_inspect,
            commands::csvimport::csv_reinspect,
            commands::csvimport::csv_import,
            commands::bak::bak_inspect,
            commands::bak::bak_discard_staged,
            commands::bak::bak_restore,
            commands::bak::bak_backup,
            commands::meta::meta_objects,
            commands::meta::meta_columns,
            commands::meta::meta_script_object,
            commands::meta::script_generate,
            commands::meta::meta_completion_schema,
        ])
        .build(tauri::generate_context!())
        .expect("error building bakehouse")
        .run(|app, event| {
            if let tauri::RunEvent::ExitRequested { .. } = event {
                // Leave nothing running: stop containers, then the runtime services.
                let state = app.state::<AppState>();
                let instances = state.instances.clone();
                let runtime = state.runtime.clone();
                let teardown = async move {
                    instances.stop_all().await;
                    if let Err(e) = runtime.shutdown().await {
                        tracing::warn!("exit teardown: {e}");
                    }
                };
                let _ = tauri::async_runtime::block_on(async {
                    tokio::time::timeout(std::time::Duration::from_secs(15), teardown).await
                });
                tracing::info!("bakehouse exiting");
            }
        });
}
