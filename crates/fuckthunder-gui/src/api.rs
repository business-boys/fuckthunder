//! Embedded bearer-authenticated HTTP API bound exclusively to loopback.

use std::{
    fmt,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, Path, Query, Request, State},
    http::{header, HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use futures::{stream, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::{runtime::Runtime, sync::oneshot, task::JoinHandle};

use crate::{
    check_account, parse_proxy, CheckStatus, ImportSummary, ProxyRecord, RunSummary,
    SessionOptions, Store, StoredAccount, WorkspaceSettings,
};

const MAX_IMPORT_BYTES: usize = 1024 * 1024;

/// Runtime state of the embedded HTTP server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApiStatus {
    /// Whether the server task is still running.
    pub running: bool,
    /// Loopback address on which the server is listening.
    pub address: SocketAddr,
}

/// Failure to start or gracefully stop the embedded HTTP server.
#[derive(Debug)]
pub enum ApiError {
    /// API startup was requested while `api_enabled` was false.
    Disabled,
    /// An empty bearer token cannot protect authenticated routes.
    EmptyToken,
    /// Loopback socket or HTTP serving failure.
    Io(std::io::Error),
    /// The Tokio server task failed.
    Task(String),
}

impl fmt::Display for ApiError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Disabled => formatter.write_str("local API is disabled in workspace settings"),
            Self::EmptyToken => formatter.write_str("local API bearer token is empty"),
            Self::Io(error) => write!(formatter, "local API I/O error: {error}"),
            Self::Task(error) => write!(formatter, "local API task failed: {error}"),
        }
    }
}

impl std::error::Error for ApiError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<std::io::Error> for ApiError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

/// A running loopback HTTP server. Dropping it initiates graceful shutdown.
pub struct ApiServer {
    address: SocketAddr,
    shutdown: Option<oneshot::Sender<()>>,
    task: JoinHandle<std::io::Result<()>>,
}

impl ApiServer {
    /// Starts the API on `127.0.0.1` using the caller's existing Tokio runtime.
    ///
    /// This performs only synchronous socket setup and does not block the runtime.
    pub fn start(
        store: Arc<Store>,
        settings: &WorkspaceSettings,
        runtime: &Runtime,
    ) -> Result<Self, ApiError> {
        if !settings.api_enabled {
            return Err(ApiError::Disabled);
        }
        if settings.api_token.is_empty() {
            return Err(ApiError::EmptyToken);
        }
        let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), settings.api_port);
        let listener = std::net::TcpListener::bind(address)?;
        listener.set_nonblocking(true)?;
        let _runtime_context = runtime.enter();
        let listener = tokio::net::TcpListener::from_std(listener)?;
        let (shutdown, receiver) = oneshot::channel();
        let app = api_router(store, settings.api_token.clone());
        let task = runtime.spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = receiver.await;
                })
                .await
        });
        Ok(Self {
            address,
            shutdown: Some(shutdown),
            task,
        })
    }

    /// Returns current task state and the bound loopback address.
    #[must_use]
    pub fn status(&self) -> ApiStatus {
        ApiStatus {
            running: !self.task.is_finished(),
            address: self.address,
        }
    }

    /// Gracefully stops accepting requests and waits for active handlers.
    pub async fn stop(mut self) -> Result<(), ApiError> {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        self.task
            .await
            .map_err(|error| ApiError::Task(error.to_string()))??;
        Ok(())
    }
}

#[derive(Clone)]
struct ApiState {
    store: Arc<Store>,
    check_active: Arc<AtomicBool>,
}

/// Builds the API router without opening a socket, primarily for integration testing.
///
/// The bearer token is required for every route except `GET /health`.
pub fn api_router(store: Arc<Store>, bearer_token: impl Into<String>) -> Router {
    let state = ApiState {
        store,
        check_active: Arc::new(AtomicBool::new(false)),
    };
    let protected = Router::new()
        .route("/api/v1/stats", get(stats))
        .route(
            "/api/v1/accounts",
            get(accounts).post(import_accounts).delete(clear_accounts),
        )
        .route("/api/v1/accounts/import", post(import_accounts))
        .route("/api/v1/accounts/{id}", delete(delete_account))
        .route(
            "/api/v1/proxies",
            get(proxies).post(import_proxies).delete(clear_proxies),
        )
        .route("/api/v1/proxies/{id}", delete(delete_proxy))
        .route("/api/v1/runs", get(runs))
        .route("/api/v1/results/reset", post(reset_results))
        .route("/api/v1/history", delete(clear_history))
        .route("/api/v1/checks", post(start_checks))
        .route_layer(middleware::from_fn_with_state(
            bearer_token.into(),
            authorize,
        ));
    Router::new()
        .route("/health", get(health))
        .merge(protected)
        .layer(DefaultBodyLimit::max(MAX_IMPORT_BYTES))
        .with_state(state)
}

async fn authorize(
    State(expected): State<String>,
    headers: HeaderMap,
    request: Request,
    next: Next,
) -> Response {
    let authorized = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .is_some_and(|provided| provided == expected);
    if authorized {
        next.run(request).await
    } else {
        error(StatusCode::UNAUTHORIZED, "invalid or missing bearer token")
    }
}

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

#[derive(Serialize)]
struct StatsResponse {
    accounts: usize,
    valid: usize,
    invalid: usize,
    errors: usize,
    checking: usize,
    idle: usize,
    proxies: usize,
    runs: usize,
}

async fn stats(State(state): State<ApiState>) -> Response {
    let accounts = match state.store.accounts() {
        Ok(accounts) => accounts,
        Err(error) => return storage_error(error),
    };
    let proxies = match state.store.proxies() {
        Ok(proxies) => proxies,
        Err(error) => return storage_error(error),
    };
    let runs = match state.store.runs(usize::MAX) {
        Ok(runs) => runs,
        Err(error) => return storage_error(error),
    };
    let count = |predicate: fn(&CheckStatus) -> bool| {
        accounts
            .iter()
            .filter(|account| predicate(&account.status))
            .count()
    };
    Json(StatsResponse {
        accounts: accounts.len(),
        valid: count(|status| matches!(status, CheckStatus::Valid { .. })),
        invalid: count(|status| matches!(status, CheckStatus::Invalid { .. })),
        errors: count(|status| matches!(status, CheckStatus::Error { .. })),
        checking: count(|status| matches!(status, CheckStatus::Checking)),
        idle: count(|status| matches!(status, CheckStatus::Idle)),
        proxies: proxies.len(),
        runs: runs.len(),
    })
    .into_response()
}

#[derive(Serialize)]
struct AccountResponse {
    id: i64,
    email: String,
    imap_host: String,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    messages: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    last_checked_at: Option<i64>,
    proxy_id: Option<i64>,
}

impl AccountResponse {
    fn new(record: StoredAccount, proxies: &[ProxyRecord]) -> Self {
        let (status, messages, reason) = match &record.status {
            CheckStatus::Idle => ("idle", None, None),
            CheckStatus::Checking => ("checking", None, None),
            CheckStatus::Valid { messages } => ("valid", Some(*messages), None),
            CheckStatus::Invalid { reason } => ("invalid", None, Some(reason)),
            CheckStatus::Error { reason } => ("error", None, Some(reason)),
        };
        let error = reason.map(|reason| redact_reason(reason.clone(), &record, proxies));
        Self {
            id: record.id,
            email: record.account.email.clone(),
            imap_host: record.account.imap_host(),
            status,
            messages,
            error,
            last_checked_at: record.last_checked_at,
            proxy_id: record.proxy_id,
        }
    }
}

async fn accounts(State(state): State<ApiState>) -> Response {
    let proxies = match state.store.proxies() {
        Ok(proxies) => proxies,
        Err(error) => return storage_error(error),
    };
    match state.store.accounts() {
        Ok(accounts) => Json(
            accounts
                .into_iter()
                .map(|account| AccountResponse::new(account, &proxies))
                .collect::<Vec<_>>(),
        )
        .into_response(),
        Err(error) => storage_error(error),
    }
}

#[derive(Serialize)]
struct ProxyResponse {
    id: i64,
    endpoint: String,
    last_used_at: Option<i64>,
}

async fn proxies(State(state): State<ApiState>) -> Response {
    match state.store.proxies() {
        Ok(proxies) => Json(
            proxies
                .into_iter()
                .map(|proxy| ProxyResponse {
                    id: proxy.id,
                    endpoint: proxy.endpoint(),
                    last_used_at: proxy.last_used_at,
                })
                .collect::<Vec<_>>(),
        )
        .into_response(),
        Err(error) => storage_error(error),
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ImportRequest {
    Text(String),
    Structured { text: String },
}

async fn import_accounts(State(state): State<ApiState>, body: Bytes) -> Response {
    import(body, move |text| state.store.import_accounts(text))
}

async fn import_proxies(State(state): State<ApiState>, body: Bytes) -> Response {
    import(body, move |text| state.store.import_proxies(text))
}

fn import(
    body: Bytes,
    operation: impl FnOnce(&str) -> rusqlite::Result<ImportSummary>,
) -> Response {
    let text = match decode_import(&body) {
        Ok(text) => text,
        Err(message) => return error(StatusCode::BAD_REQUEST, message),
    };
    match operation(&text) {
        Ok(summary) => Json(json!({
            "imported": summary.imported,
            "duplicates": summary.duplicates,
            "invalid": summary.invalid,
        }))
        .into_response(),
        Err(error) => storage_error(error),
    }
}

fn decode_import(body: &[u8]) -> Result<String, &'static str> {
    let text = std::str::from_utf8(body).map_err(|_| "request body must be UTF-8")?;
    let trimmed = text.trim_start();
    if trimmed.starts_with('{') || trimmed.starts_with('"') {
        return match serde_json::from_str::<ImportRequest>(trimmed) {
            Ok(ImportRequest::Text(text) | ImportRequest::Structured { text }) => Ok(text),
            Err(_) => Err("expected a JSON string or an object with a text field"),
        };
    }
    Ok(text.to_owned())
}

async fn delete_account(State(state): State<ApiState>, Path(account_id): Path<i64>) -> Response {
    match state.store.delete_account(account_id) {
        Ok(()) => Json(json!({ "ok": true })).into_response(),
        Err(error) => storage_error(error),
    }
}

async fn clear_accounts(State(state): State<ApiState>) -> Response {
    match state.store.clear_accounts() {
        Ok(deleted) => Json(json!({ "deleted": deleted })).into_response(),
        Err(error) => storage_error(error),
    }
}

async fn delete_proxy(State(state): State<ApiState>, Path(proxy_id): Path<i64>) -> Response {
    match state.store.delete_proxy(proxy_id) {
        Ok(()) => Json(json!({ "ok": true })).into_response(),
        Err(error) => storage_error(error),
    }
}

async fn clear_proxies(State(state): State<ApiState>) -> Response {
    match state.store.clear_proxies() {
        Ok(deleted) => Json(json!({ "deleted": deleted })).into_response(),
        Err(error) => storage_error(error),
    }
}

#[derive(Deserialize)]
struct RunQuery {
    limit: Option<usize>,
}

async fn runs(State(state): State<ApiState>, Query(query): Query<RunQuery>) -> Response {
    match state.store.runs(query.limit.unwrap_or(100).clamp(1, 1000)) {
        Ok(runs) => Json(runs.into_iter().map(run_json).collect::<Vec<_>>()).into_response(),
        Err(error) => storage_error(error),
    }
}

fn run_json(run: RunSummary) -> Value {
    json!({
        "id": run.id,
        "started_at": run.started_at,
        "finished_at": run.finished_at,
        "total": run.total,
        "valid": run.valid,
        "invalid": run.invalid,
        "errors": run.errors,
    })
}

async fn reset_results(State(state): State<ApiState>) -> Response {
    match state.store.reset_results() {
        Ok(()) => Json(json!({ "ok": true })).into_response(),
        Err(error) => storage_error(error),
    }
}

async fn clear_history(State(state): State<ApiState>) -> Response {
    match state.store.clear_history() {
        Ok(deleted) => Json(json!({ "deleted": deleted })).into_response(),
        Err(error) => storage_error(error),
    }
}

struct ActiveCheckGuard(Arc<AtomicBool>);

impl Drop for ActiveCheckGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

async fn start_checks(State(state): State<ApiState>) -> Response {
    if state
        .check_active
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return error(StatusCode::CONFLICT, "an API check run is already active");
    }
    let _guard = ActiveCheckGuard(state.check_active.clone());
    let accounts = match state.store.accounts() {
        Ok(accounts) => accounts,
        Err(error) => return storage_error(error),
    };
    if accounts.is_empty() {
        return error(StatusCode::BAD_REQUEST, "no accounts to check");
    }
    let proxies = match state.store.proxies() {
        Ok(proxies) => proxies,
        Err(error) => return storage_error(error),
    };
    let settings = match state.store.workspace_settings() {
        Ok(settings) => settings,
        Err(error) => return storage_error(error),
    };
    let run_id = match state.store.begin_run() {
        Ok(run_id) => run_id,
        Err(error) => return storage_error(error),
    };
    let jobs = accounts
        .into_iter()
        .enumerate()
        .map(|(position, account)| {
            let proxy = if proxies.is_empty() {
                None
            } else {
                proxies.get(position % proxies.len())
            };
            let options = SessionOptions {
                proxy: proxy.and_then(|record| parse_proxy(&record.url).ok()),
                timeout: Duration::from_secs(settings.timeout_seconds.max(1)),
            };
            (
                account.id,
                account.account,
                proxy.map(|record| record.id),
                options,
            )
        })
        .collect::<Vec<_>>();
    let store = state.store.clone();
    let concurrency = settings.concurrency.clamp(1, 256);
    tokio::spawn(async move {
        let _guard = _guard;
        let results = stream::iter(jobs.into_iter().map(
            |(account_id, account, proxy_id, options)| async move {
                let status = check_account(&account, &options).await;
                (account_id, proxy_id, status)
            },
        ))
        .buffer_unordered(concurrency)
        .collect::<Vec<_>>()
        .await;
        for (account_id, proxy_id, status) in results {
            let store = store.clone();
            let _ = tokio::task::spawn_blocking(move || {
                store.record_check(run_id, account_id, proxy_id, &status)
            })
            .await;
        }
        let _ = tokio::task::spawn_blocking(move || store.finish_run(run_id)).await;
    });
    (StatusCode::ACCEPTED, Json(json!({ "run_id": run_id }))).into_response()
}

fn redact_reason(mut reason: String, account: &StoredAccount, proxies: &[ProxyRecord]) -> String {
    reason = reason.replace(&account.account.password, "<redacted>");
    for proxy in proxies {
        reason = reason.replace(&proxy.url, &proxy.endpoint());
        if let Ok(url) = url::Url::parse(&proxy.url) {
            if !url.username().is_empty() {
                reason = reason.replace(url.username(), "<redacted>");
            }
            if let Some(password) = url.password().filter(|password| !password.is_empty()) {
                reason = reason.replace(password, "<redacted>");
            }
        }
    }
    reason
}

fn storage_error(storage: rusqlite::Error) -> Response {
    if matches!(storage, rusqlite::Error::InvalidQuery) {
        error(
            StatusCode::CONFLICT,
            "stop the active check run before changing workspace data",
        )
    } else {
        error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "local storage operation failed",
        )
    }
}

fn error(status: StatusCode, message: impl Into<String>) -> Response {
    (status, Json(json!({ "error": message.into() }))).into_response()
}

#[cfg(test)]
mod tests {
    use axum::{
        body::Body,
        http::{Method, Request},
    };
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use super::*;

    fn authorized(method: Method, uri: &str, body: Body) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .header(header::AUTHORIZATION, "Bearer test-token")
            .body(body)
            .unwrap()
    }

    async fn json_body(response: Response) -> Value {
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn health_is_public_and_other_routes_require_bearer_auth() {
        let app = api_router(Arc::new(Store::open_in_memory().unwrap()), "test-token");
        let health = app
            .clone()
            .oneshot(Request::get("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(health.status(), StatusCode::OK);
        let denied = app
            .clone()
            .oneshot(Request::get("/api/v1/stats").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
        let accepted = app
            .oneshot(authorized(Method::GET, "/api/v1/stats", Body::empty()))
            .await
            .unwrap();
        assert_eq!(accepted.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn imports_manage_data_and_never_serialize_secrets() {
        let store = Arc::new(Store::open_in_memory().unwrap());
        store.import_accounts("a@web.de:supersecret").unwrap();
        store
            .import_proxies("http://proxy-user:proxy-pass@proxy.example:8080")
            .unwrap();
        let account = store.accounts().unwrap().remove(0);
        let proxy = store.proxies().unwrap().remove(0);
        let run = store.begin_run().unwrap();
        store
            .record_check(
                run,
                account.id,
                Some(proxy.id),
                &CheckStatus::Error {
                    reason: "supersecret http://proxy-user:proxy-pass@proxy.example:8080".into(),
                },
            )
            .unwrap();
        store.finish_run(run).unwrap();
        let app = api_router(store.clone(), "test-token");

        let response = app
            .clone()
            .oneshot(authorized(Method::GET, "/api/v1/accounts", Body::empty()))
            .await
            .unwrap();
        let body = json_body(response).await.to_string();
        assert!(!body.contains("supersecret"));
        assert!(!body.contains("proxy-user"));
        assert!(!body.contains("proxy-pass"));
        assert!(!body.contains("proxy-user:proxy-pass"));
        assert!(body.contains("<redacted>"));

        let response = app
            .clone()
            .oneshot(authorized(Method::GET, "/api/v1/proxies", Body::empty()))
            .await
            .unwrap();
        let body = json_body(response).await;
        assert_eq!(body[0]["endpoint"], "http://proxy.example:8080");
        let body = body.to_string();
        assert!(!body.contains("proxy-user"));
        assert!(!body.contains("proxy-pass"));

        let response = app
            .clone()
            .oneshot(authorized(
                Method::POST,
                "/api/v1/accounts",
                Body::from("{\"text\":\"b@gmx.de:another-secret\"}"),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(json_body(response).await["imported"], 1);
        assert_eq!(store.accounts().unwrap().len(), 2);

        let response = app
            .clone()
            .oneshot(authorized(Method::DELETE, "/api/v1/history", Body::empty()))
            .await
            .unwrap();
        assert_eq!(json_body(response).await["deleted"], 1);
        let response = app
            .clone()
            .oneshot(authorized(
                Method::DELETE,
                "/api/v1/accounts",
                Body::empty(),
            ))
            .await
            .unwrap();
        assert_eq!(json_body(response).await["deleted"], 2);
        let response = app
            .oneshot(authorized(Method::DELETE, "/api/v1/proxies", Body::empty()))
            .await
            .unwrap();
        assert_eq!(json_body(response).await["deleted"], 1);
    }

    #[test]
    fn starts_and_stops_on_an_existing_runtime() {
        let store = Arc::new(Store::open_in_memory().unwrap());
        let mut settings = store.workspace_settings().unwrap();
        settings.api_enabled = true;
        settings.api_port = 0;
        let runtime = Runtime::new().unwrap();
        let server = ApiServer::start(store, &settings, &runtime).unwrap();
        assert_eq!(
            server.status().address.ip(),
            IpAddr::V4(Ipv4Addr::LOCALHOST)
        );
        runtime.block_on(server.stop()).unwrap();
    }
}
