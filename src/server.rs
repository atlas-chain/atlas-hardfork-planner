use std::sync::Arc;
use std::time::Instant;

use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use serde_json::{Value, json};
use tokio::net::TcpListener;

use crate::frontend::INDEX_HTML;
use crate::model::ScheduleEntry;
use crate::store::{RemoveForkFailure, ScheduleStore, Snapshot};

#[derive(Clone, Debug)]
pub struct AppState {
    pub store: Arc<ScheduleStore>,
    pub schedule_path: Arc<String>,
    pub admin_key: Option<Arc<String>>,
}

pub async fn run_server(state: AppState, listen_host: String, listen_port: u16) {
    let bind_address = format!("{listen_host}:{listen_port}");

    let listener = match TcpListener::bind(&bind_address).await {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("failed to bind HTTP server on {bind_address}: {error}");
            return;
        }
    };

    let snapshot = state.store.snapshot();
    println!(
        "{}",
        json!({
            "message": "arkiv protocol schedule service listening",
            "url": format!("http://{bind_address}/arkiv-protocol-schedule.json"),
            "ui": format!("http://{bind_address}/"),
            "chainId": snapshot.chain_id,
            "version": snapshot.version,
            "currentBlock": snapshot.current_block,
            "admin": state.admin_key.is_some(),
            "endpoints": ["/", "/status", "/arkiv-protocol-schedule.json", "/healthz", "/admin/forks"],
        })
    );

    let app = Router::new()
        .route("/", get(index_handler))
        .route("/status", get(status_handler))
        .route("/arkiv-protocol-schedule.json", get(schedule_handler))
        .route("/healthz", get(health_handler))
        .route("/admin/forks", post(admin_add_fork))
        .route("/admin/forks/{activation_block}", delete(admin_remove_fork))
        .fallback(not_found_handler)
        .with_state(state);

    if let Err(error) = axum::serve(listener, app).await {
        eprintln!("HTTP server failed: {error}");
    }
}

async fn index_handler() -> Response {
    (
        StatusCode::OK,
        [("content-type", "text/html; charset=utf-8")],
        INDEX_HTML,
    )
        .into_response()
}

async fn schedule_handler(State(state): State<AppState>) -> Response {
    let started = Instant::now();
    let snapshot = state.store.snapshot();
    let latency_ms = started.elapsed().as_millis();

    println!(
        "{}",
        json!({
            "message": "served schedule",
            "path": "/arkiv-protocol-schedule.json",
            "status": 200,
            "chainId": snapshot.chain_id,
            "version": snapshot.version,
            "currentBlock": snapshot.current_block,
            "hash": snapshot.hash,
            "latencyMs": latency_ms.to_string(),
        })
    );

    (
        StatusCode::OK,
        [("content-type", "application/json"), ("cache-control", "no-cache")],
        snapshot.canonical,
    )
        .into_response()
}

async fn health_handler(State(state): State<AppState>) -> Json<Value> {
    let snapshot = state.store.snapshot();
    let mut body = json!({
        "ok": true,
        "chainId": snapshot.chain_id,
        "version": snapshot.version,
    });

    if let Some(current_block) = snapshot.current_block {
        body["currentBlock"] = json!(current_block);
    }

    Json(body)
}

async fn status_handler(State(state): State<AppState>) -> Json<Value> {
    let snapshot = state.store.snapshot();
    let releases: Vec<Value> = state
        .store
        .history()
        .into_iter()
        .rev()
        .map(|record| {
            json!({
                "version": record.version,
                "chainId": record.chain_id,
                "currentBlock": record.current_block,
                "activeEntries": record.active_entries,
                "hash": record.hash,
                "installedAt": record.installed_at,
            })
        })
        .collect();

    Json(json!({
        "ok": true,
        "service": "arkiv-protocol-schedule",
        "chainId": snapshot.chain_id,
        "version": snapshot.version,
        "currentBlock": snapshot.current_block,
        "activeEntries": snapshot.active_entries,
        "retainedVersions": snapshot.retained_versions,
        "hash": snapshot.hash,
        "admin": state.admin_key.is_some(),
        "releases": releases,
        "endpoints": ["/", "/status", "/arkiv-protocol-schedule.json", "/healthz", "/admin/forks"],
    }))
}

async fn admin_add_fork(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(entry): Json<ScheduleEntry>,
) -> Response {
    if let Err(response) = authorize(&state, &headers) {
        return *response;
    }

    match state.store.add_fork(entry) {
        Ok(snapshot) => publish(&state, snapshot, "fork added"),
        Err(failure) => error_response(StatusCode::BAD_REQUEST, failure.to_string()),
    }
}

async fn admin_remove_fork(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(activation_block): Path<u64>,
) -> Response {
    if let Err(response) = authorize(&state, &headers) {
        return *response;
    }

    match state.store.remove_fork(activation_block) {
        Ok(snapshot) => publish(&state, snapshot, "fork removed"),
        Err(RemoveForkFailure::NotFound) => error_response(
            StatusCode::NOT_FOUND,
            format!("no fork at activation block {activation_block}"),
        ),
        Err(RemoveForkFailure::Validation(failure)) => {
            error_response(StatusCode::BAD_REQUEST, failure.to_string())
        }
    }
}

async fn not_found_handler() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(json!({ "ok": false, "error": { "message": "Not found" } })),
    )
        .into_response()
}

fn authorize(state: &AppState, headers: &HeaderMap) -> Result<(), Box<Response>> {
    let Some(key) = state.admin_key.as_ref() else {
        return Err(Box::new(error_response(
            StatusCode::FORBIDDEN,
            "admin disabled; set ADMIN_BEARER_KEY to enable",
        )));
    };

    let Some(provided) = bearer_token(headers) else {
        return Err(Box::new(error_response(
            StatusCode::UNAUTHORIZED,
            "missing or malformed Authorization header",
        )));
    };

    if !constant_time_eq(provided.as_bytes(), key.as_bytes()) {
        return Err(Box::new(error_response(
            StatusCode::UNAUTHORIZED,
            "invalid bearer key",
        )));
    }

    Ok(())
}

fn bearer_token(headers: &HeaderMap) -> Option<String> {
    let raw = headers.get("authorization")?.to_str().ok()?;
    let value = raw.strip_prefix("Bearer ")?;
    Some(value.trim().to_string())
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in left.iter().zip(right.iter()) {
        diff |= a ^ b;
    }
    diff == 0
}

fn publish(state: &AppState, snapshot: Snapshot, action: &str) -> Response {
    let path = state.schedule_path.as_str();
    let version = snapshot.version;
    let hash = snapshot.hash.clone();

    if let Err(error) = std::fs::write(path, &snapshot.canonical) {
        eprintln!(
            "{}",
            json!({
                "message": "failed to persist schedule",
                "path": path,
                "error": error.to_string(),
            })
        );
    }

    println!(
        "{}",
        json!({
            "message": action,
            "path": "/admin/forks",
            "schedulePath": path,
            "version": version,
            "hash": hash,
        })
    );

    (
        StatusCode::OK,
        [("content-type", "application/json"), ("cache-control", "no-cache")],
        snapshot.canonical,
    )
        .into_response()
}

fn error_response<S>(status: StatusCode, message: S) -> Response
where
    S: AsRef<str>,
{
    (
        status,
        Json(json!({ "ok": false, "error": { "message": message.as_ref() } })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ScheduleDocument, ScheduleEntry};
    use crate::store::ScheduleStore;

    fn sample_entry() -> ScheduleEntry {
        ScheduleEntry {
            activation_block: 0,
            min_base_fee_per_gas: "440000000".to_string(),
            elasticity_multiplier: 2,
            base_fee_max_change_denominator: 8,
            max_block_gas_limit: "30000000".to_string(),
        }
    }

    fn sample_document() -> ScheduleDocument {
        ScheduleDocument {
            chain_id: 42069,
            version: 1,
            current_block: None,
            schedule: vec![sample_entry()],
        }
    }

    fn state_with_key(key: Option<&str>) -> AppState {
        AppState {
            store: Arc::new(ScheduleStore::new(sample_document(), None).expect("valid document")),
            schedule_path: Arc::new("/tmp/unused-arkiv-schedule.json".to_string()),
            admin_key: key.map(|k| Arc::new(k.to_string())),
        }
    }

    async fn body_string(response: Response) -> (StatusCode, axum::http::HeaderMap, String) {
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body bytes");
        let body = String::from_utf8(bytes.to_vec()).expect("utf8 body");
        (status, headers, body)
    }

    #[test]
    fn constant_time_eq_handles_inputs() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
    }

    #[test]
    fn bearer_token_parses_header() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer s3cret".parse().unwrap());
        assert_eq!(bearer_token(&headers).as_deref(), Some("s3cret"));
        assert_eq!(bearer_token(&HeaderMap::new()), None);
    }

    #[tokio::test]
    async fn index_handler_serves_html() {
        let (status, headers, body) = body_string(index_handler().await).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers.get("content-type").unwrap(), "text/html; charset=utf-8");
        assert!(body.contains("Arkiv Hardfork Planner"));
    }

    #[tokio::test]
    async fn schedule_handler_serves_canonical_json() {
        let state = state_with_key(None);
        let response = schedule_handler(State(state)).await;
        let (status, headers, body) = body_string(response).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers.get("content-type").unwrap(), "application/json");
        assert_eq!(headers.get("cache-control").unwrap(), "no-cache");
        assert!(body.contains("\"chainId\": 42069"));
    }

    #[tokio::test]
    async fn health_handler_reports_status() {
        let state = state_with_key(None);
        let Json(body) = health_handler(State(state)).await;
        assert_eq!(body["ok"], json!(true));
        assert_eq!(body["version"], json!(1));
    }

    #[tokio::test]
    async fn admin_is_disabled_without_key() {
        let state = state_with_key(None);
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer anything".parse().unwrap());

        let response = admin_add_fork(State(state), headers, Json(sample_entry())).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn admin_rejects_wrong_bearer() {
        let state = state_with_key(Some("real-key"));
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer wrong".parse().unwrap());

        let response = admin_add_fork(State(state), headers, Json(sample_entry())).await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn admin_add_fork_persists_and_returns_document() {
        let temp = std::env::temp_dir().join("arkiv-admin-add-test.json");
        std::fs::write(&temp, "previous").unwrap();
        let document = sample_document();
        let state = AppState {
            store: Arc::new(ScheduleStore::new(document, None).expect("valid")),
            schedule_path: Arc::new(temp.to_string_lossy().to_string()),
            admin_key: Some(Arc::new("real-key".to_string())),
        };
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer real-key".parse().unwrap());

        let mut entry = sample_entry();
        entry.activation_block = 1_000;
        let response = admin_add_fork(State(state.clone()), headers, Json(entry)).await;
        let (status, _, body) = body_string(response).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("\"version\": 2"));
        assert!(body.contains("\"activationBlock\": 1000"));

        let persisted = std::fs::read_to_string(state.schedule_path.as_str()).unwrap();
        assert!(persisted.contains("\"activationBlock\": 1000"));
        let _ = std::fs::remove_file(state.schedule_path.as_str());
    }
}
