mod config;
mod frontend;
mod model;
mod quantity;
mod rpc;
mod server;
mod store;
mod validation;

use std::ffi::{OsStr, OsString};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use config::create_config;
use model::ScheduleDocument;
use store::ScheduleStore;

const WATCH_INTERVAL_SECONDS: u64 = 5;

fn main() {
    install_process_panic_handler();

    match startup_action(std::env::args_os().skip(1)) {
        StartupAction::Run => {}
        StartupAction::PrintVersion => {
            println!("{}", env!("CARGO_PKG_VERSION"));
            return;
        }
        StartupAction::Error(message) => {
            eprintln!("{message}");
            std::process::exit(2);
        }
    }

    let config = create_config();

    let raw_schedule = std::fs::read_to_string(&config.schedule_path).unwrap_or_else(|error| {
        panic!("failed to read schedule file {}: {error}", config.schedule_path)
    });
    let document: ScheduleDocument = serde_json::from_str(&raw_schedule)
        .unwrap_or_else(|error| panic!("failed to parse schedule JSON: {error}"));
    let store = Arc::new(
        ScheduleStore::new(document, config.chain_id)
            .unwrap_or_else(|error| panic!("schedule validation failed: {error}")),
    );

    let admin_key = config
        .admin_bearer_key
        .clone()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    {
        let snapshot = store.snapshot();
        println!(
            "{}",
            serde_json::json!({
                "message": "loaded schedule",
                "path": config.schedule_path,
                "chainId": snapshot.chain_id,
                "version": snapshot.version,
                "hash": snapshot.hash,
                "rpcUrl": config.rpc_url,
                "admin": admin_key.is_some(),
            })
        );
    }

    let worker_threads = config.web_workers.get();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(worker_threads)
        .enable_io()
        .enable_time()
        .build()
        .expect("failed to build tokio runtime");

    runtime.block_on(async move {
        if let Some(rpc_url) = config.rpc_url.clone() {
            let timeout = Duration::from_millis(config.rpc_timeout_ms.get());
            let interval = Duration::from_secs(config.rpc_poll_seconds.get());
            let client = reqwest::Client::builder()
                .timeout(timeout)
                .build()
                .expect("failed to build reqwest client");

            let poll_store = Arc::clone(&store);
            tokio::spawn(async move {
                rpc::poll_loop(poll_store, client, rpc_url, timeout, interval).await;
            });
        } else {
            let watch_store = Arc::clone(&store);
            let watch_path = config.schedule_path.clone();
            tokio::spawn(async move {
                watch_schedule_file(watch_store, watch_path).await;
            });
        }

        let app_state = server::AppState {
            store,
            schedule_path: Arc::new(config.schedule_path.clone()),
            admin_key: admin_key.map(Arc::new),
        };

        server::run_server(app_state, config.listen_host.clone(), config.listen_port.get()).await;
    });
}

async fn watch_schedule_file(store: Arc<ScheduleStore>, path: String) {
    let mut last_mtime = current_mtime(&path);

    loop {
        tokio::time::sleep(Duration::from_secs(WATCH_INTERVAL_SECONDS)).await;

        let mtime = current_mtime(&path);
        if mtime == last_mtime {
            continue;
        }
        last_mtime = mtime;

        match reload_schedule(&path) {
            Ok(document) => {
                let version = document.version;
                match store.install(document) {
                    Ok(()) => {
                        let snapshot = store.snapshot();
                        println!(
                            "{}",
                            serde_json::json!({
                                "message": "schedule reloaded",
                                "path": path,
                                "version": snapshot.version,
                                "hash": snapshot.hash,
                            })
                        );
                    }
                    Err(error) => {
                        eprintln!(
                            "{}",
                            serde_json::json!({
                                "message": "schedule reload rejected, keeping last good",
                                "path": path,
                                "offeredVersion": version,
                                "error": error.to_string(),
                            })
                        );
                    }
                }
            }
            Err(error) => {
                eprintln!(
                    "{}",
                    serde_json::json!({
                        "message": "schedule reload failed, keeping last good",
                        "path": path,
                        "error": error,
                    })
                );
            }
        }
    }
}

fn current_mtime(path: &str) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|metadata| metadata.modified()).ok()
}

fn reload_schedule(path: &str) -> Result<ScheduleDocument, String> {
    let raw = std::fs::read_to_string(path).map_err(|error| format!("read failed: {error}"))?;
    serde_json::from_str::<ScheduleDocument>(&raw).map_err(|error| format!("parse failed: {error}"))
}

fn install_process_panic_handler() {
    std::panic::set_hook(Box::new(|panic_info| {
        eprintln!("uncaught panic: {panic_info}");
    }));
}

#[derive(Debug, PartialEq, Eq)]
enum StartupAction {
    Run,
    PrintVersion,
    Error(String),
}

fn startup_action<I>(args: I) -> StartupAction
where
    I: IntoIterator<Item = OsString>,
{
    let mut saw_version = false;

    for arg in args {
        if arg == OsStr::new("-v") || arg == OsStr::new("--version") {
            saw_version = true;
            continue;
        }

        return StartupAction::Error(format!(
            "unsupported command-line argument: {}. Use environment variables to configure arkiv-hardfork-planner; command-line arguments are not supported.",
            arg.to_string_lossy()
        ));
    }

    if saw_version {
        StartupAction::PrintVersion
    } else {
        StartupAction::Run
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn no_arguments_runs_service() {
        assert_eq!(startup_action(args(&[])), StartupAction::Run);
    }

    #[test]
    fn version_arguments_print_version() {
        assert_eq!(startup_action(args(&["-v"])), StartupAction::PrintVersion);
        assert_eq!(
            startup_action(args(&["--version"])),
            StartupAction::PrintVersion
        );
    }

    #[test]
    fn invalid_argument_returns_error() {
        match startup_action(args(&["--rpc-url", "http://localhost:8545"])) {
            StartupAction::Error(message) => {
                assert!(message.contains("--rpc-url"));
                assert!(message.contains("environment variables"));
            }
            action => panic!("expected error action, got {action:?}"),
        }
    }

    #[test]
    fn version_with_invalid_argument_returns_error() {
        assert!(matches!(
            startup_action(args(&["--version", "--rpc-url"])),
            StartupAction::Error(_)
        ));
    }
}
