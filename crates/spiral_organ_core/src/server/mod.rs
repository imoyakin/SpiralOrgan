use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader as StdBufReader};
use std::path::{Component, Path as FsPath, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use agent_role_pipeline::{
    PipelineRunInput, RolePipelineRunner, RoleStageExecutor, StageExecutionContext,
    StageExecutionOutput, StageKind, ensure_default_pipeline_file,
};
use async_trait::async_trait;
use axum::body::{Body, to_bytes};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::{Method, Request, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader as AsyncBufReader};
use tokio::process::Command as TokioCommand;
use tokio::sync::{broadcast, mpsc};
use tower::util::ServiceExt;

use crate::domain::{ChangeFile, ChangeStatus, ModelRequest};
use crate::local_client;
use crate::runtime_config::{self, NewProjectDispatcher, NewProvider};
use crate::runtime_provider::RuntimeProvider;
use crate::traits::Provider;

mod provider_agent;

#[derive(Clone)]
pub(crate) struct AppState {
    inner: Arc<Mutex<InMemoryApiState>>,
    event_tx: broadcast::Sender<RealtimeEvent>,
    persistence_path: PathBuf,
}

impl Default for AppState {
    fn default() -> Self {
        let (event_tx, _event_rx) = broadcast::channel(512);
        let persistence_path = default_persistence_path();
        let inner = load_persisted_state(&persistence_path).unwrap_or_else(|err| {
            eprintln!("serve: state load warning: {err}");
            InMemoryApiState::default()
        });
        Self {
            inner: Arc::new(Mutex::new(inner)),
            event_tx,
            persistence_path,
        }
    }
}

impl AppState {
    fn publish_event(&self, event: RealtimeEvent) {
        let _ = self.event_tx.send(event);
    }

    fn persist(&self) {
        let snapshot = match self.inner.lock() {
            Ok(guard) => guard.clone(),
            Err(_) => {
                eprintln!("serve: state persist skipped: mutex poisoned");
                return;
            }
        };
        if let Err(err) = persist_state(&self.persistence_path, &snapshot) {
            eprintln!("serve: state persist warning: {err}");
        }
    }

    pub(crate) fn subscribe_events(
        &self,
        session_id: Option<String>,
        task_id: Option<String>,
    ) -> EventSubscription {
        EventSubscription {
            rx: self.event_tx.subscribe(),
            session_id,
            task_id,
        }
    }
}

#[derive(Clone)]
struct TaskOutputStreamContext {
    state: AppState,
    task_id: String,
    session_id: String,
}

#[derive(Debug, Clone)]
struct DispatchRawChunk {
    stream: &'static str,
    text: String,
}

#[derive(Default, Clone)]
struct InMemoryApiState {
    next_session_id: u64,
    next_task_id: u64,
    next_event_id: u64,
    next_ack_id: u64,
    next_deploy_id: u64,
    next_skill_id: u64,
    next_approval_id: u64,
    next_ssh_target_id: u64,
    sessions: HashMap<String, SessionRecord>,
    tasks: HashMap<String, TaskRecord>,
    task_events: HashMap<String, Vec<TaskEvent>>,
    session_changes: HashMap<String, Vec<ChangeFile>>,
    acked_changes: HashSet<String>,
    skills: HashMap<String, SkillRecord>,
    pending_approvals: HashMap<String, SandboxApprovalRecord>,
    ssh_targets: HashMap<String, SshTargetRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SessionRecord {
    project_id: String,
    target: String,
    dispatcher_kind: Option<String>,
    dispatcher_ref: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TaskRecord {
    task_id: String,
    session_id: String,
    title: String,
    status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TaskEvent {
    event_id: String,
    event_type: String,
    session_id: String,
    task_id: String,
    timestamp_ms: u64,
    payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SkillRecord {
    skill_id: String,
    name: String,
    description: String,
    path: String,
    active: bool,
    created_at_ms: u64,
    updated_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum SandboxOperationKind {
    FileWrite,
    FileDelete,
    DirectoryCreate,
    DirectoryDelete,
    CommandRun,
    McpCall,
}

impl SandboxOperationKind {
    fn as_str(&self) -> &'static str {
        match self {
            SandboxOperationKind::FileWrite => "file_write",
            SandboxOperationKind::FileDelete => "file_delete",
            SandboxOperationKind::DirectoryCreate => "directory_create",
            SandboxOperationKind::DirectoryDelete => "directory_delete",
            SandboxOperationKind::CommandRun => "command_run",
            SandboxOperationKind::McpCall => "mcp_call",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SandboxAction {
    operation: SandboxOperationKind,
    path: Option<String>,
    content: Option<String>,
    command: Option<String>,
    args: Vec<String>,
    timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SandboxApprovalRecord {
    approval_id: String,
    session_id: String,
    task_id: Option<String>,
    reason: String,
    action: SandboxAction,
    created_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SshTargetRecord {
    ssh_target_id: String,
    name: String,
    host: String,
    port: Option<u16>,
    username: Option<String>,
    identity_file: Option<String>,
    remote_workdir: Option<String>,
    options: Vec<String>,
    created_at_ms: u64,
    updated_at_ms: u64,
}

#[derive(Debug, Clone, Copy)]
enum SkillDirectiveAction {
    Activate,
    Deactivate,
}

impl SkillDirectiveAction {
    fn as_str(&self) -> &'static str {
        match self {
            SkillDirectiveAction::Activate => "activate",
            SkillDirectiveAction::Deactivate => "deactivate",
        }
    }
}

#[derive(Debug, Clone)]
struct SkillDirective {
    action: SkillDirectiveAction,
    target: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedChangeFile {
    path: String,
    status: String,
    additions: u32,
    deletions: u32,
    task_id: String,
    agent_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedState {
    next_session_id: u64,
    next_task_id: u64,
    next_event_id: u64,
    next_ack_id: u64,
    next_deploy_id: u64,
    #[serde(default)]
    next_skill_id: u64,
    #[serde(default)]
    next_approval_id: u64,
    #[serde(default)]
    next_ssh_target_id: u64,
    sessions: HashMap<String, SessionRecord>,
    tasks: HashMap<String, TaskRecord>,
    task_events: HashMap<String, Vec<TaskEvent>>,
    session_changes: HashMap<String, Vec<PersistedChangeFile>>,
    acked_changes: Vec<String>,
    #[serde(default)]
    skills: HashMap<String, SkillRecord>,
    #[serde(default)]
    pending_approvals: HashMap<String, SandboxApprovalRecord>,
    #[serde(default)]
    ssh_targets: HashMap<String, SshTargetRecord>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct RealtimeEvent {
    event_id: String,
    event_type: String,
    session_id: String,
    task_id: Option<String>,
    timestamp_ms: u64,
    payload: Value,
}

impl RealtimeEvent {
    fn from_task_event(event: &TaskEvent) -> Self {
        Self {
            event_id: event.event_id.clone(),
            event_type: event.event_type.clone(),
            session_id: event.session_id.clone(),
            task_id: Some(event.task_id.clone()),
            timestamp_ms: event.timestamp_ms,
            payload: event.payload.clone(),
        }
    }
}

pub(crate) struct EventSubscription {
    rx: broadcast::Receiver<RealtimeEvent>,
    session_id: Option<String>,
    task_id: Option<String>,
}

impl EventSubscription {
    pub(crate) async fn next_event(&mut self, timeout_ms: u64) -> Result<Option<Value>, String> {
        let timeout_ms = timeout_ms.max(1_000);
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_millis(timeout_ms);
        loop {
            let remaining = deadline
                .checked_duration_since(tokio::time::Instant::now())
                .unwrap_or_else(|| tokio::time::Duration::from_millis(0));
            if remaining.is_zero() {
                return Ok(None);
            }

            let received = tokio::time::timeout(remaining, self.rx.recv()).await;
            let event = match received {
                Ok(Ok(event)) => event,
                Ok(Err(broadcast::error::RecvError::Lagged(_))) => continue,
                Ok(Err(broadcast::error::RecvError::Closed)) => return Ok(None),
                Err(_) => return Ok(None),
            };

            if !event_matches_filter(&event, self.session_id.as_deref(), self.task_id.as_deref()) {
                continue;
            }

            return Ok(Some(json!({
                "event_id": event.event_id,
                "event_type": event.event_type,
                "session_id": event.session_id,
                "task_id": event.task_id,
                "timestamp_ms": event.timestamp_ms,
                "payload": event.payload
            })));
        }
    }
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
        }
    }

    fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: message.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({
                "error": self.message
            })),
        )
            .into_response()
    }
}

type ApiResult<T> = Result<Json<T>, ApiError>;

#[derive(Debug, Deserialize)]
struct OpenSessionRequest {
    project_id: String,
    target: Option<String>,
    dispatcher_kind: Option<String>,
    dispatcher_ref: Option<String>,
}

#[derive(Debug, Serialize)]
struct OpenSessionResponse {
    session_id: String,
    project_id: String,
    target: String,
    dispatcher_kind: String,
    dispatcher_ref: String,
    status: String,
}

#[derive(Debug, Deserialize)]
struct SubmitTaskRequest {
    session_id: String,
    title: String,
    input: Option<String>,
    dispatcher_kind: Option<String>,
    dispatcher_ref: Option<String>,
}

#[derive(Debug, Serialize)]
struct SubmitTaskResponse {
    task_id: String,
    status: String,
}

#[derive(Debug, Deserialize)]
struct AbortTaskRequest {
    task_id: String,
    reason: Option<String>,
}

#[derive(Debug, Serialize)]
struct AbortTaskResponse {
    task_id: String,
    status: String,
}

#[derive(Debug, Serialize)]
struct TaskStatusResponse {
    task_id: String,
    session_id: String,
    title: String,
    status: String,
}

#[derive(Debug, Serialize)]
struct TaskEventsResponse {
    task_id: String,
    events: Vec<TaskEvent>,
}

#[derive(Debug, Deserialize)]
struct DeployRequest {
    project_id: Option<String>,
    session_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct DeployResponse {
    deploy_id: String,
    status: String,
    project_id: Option<String>,
    session_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProjectSessionPath {
    project_id: String,
    session_id: String,
}

#[derive(Debug, Deserialize)]
struct FileViewQuery {
    path: String,
    view: String,
}

#[derive(Debug, Serialize)]
struct FileStatusItem {
    path: String,
    status: String,
    additions: u32,
    deletions: u32,
    task_id: String,
    agent_id: String,
}

#[derive(Debug, Serialize)]
struct FileStatusResponse {
    project_id: String,
    session_id: String,
    files: Vec<FileStatusItem>,
}

#[derive(Debug, Serialize)]
struct FileViewResponse {
    project_id: String,
    session_id: String,
    path: String,
    view: String,
    content: Value,
}

#[derive(Debug, Serialize)]
struct ChangeSummaryResponse {
    project_id: String,
    session_id: String,
    file_count: usize,
    additions: u32,
    deletions: u32,
    acked: bool,
}

#[derive(Debug, Deserialize)]
struct ChangeAckRequest {
    actor: Option<String>,
    note: Option<String>,
}

#[derive(Debug, Serialize)]
struct ChangeAckResponse {
    ack_id: String,
    project_id: String,
    session_id: String,
    actor: String,
    note: Option<String>,
    timestamp_ms: u64,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct WsQuery {
    token: Option<String>,
    session_id: Option<String>,
    task_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SkillCreateRequest {
    name: String,
    description: Option<String>,
    path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SkillPath {
    skill_id: String,
}

#[derive(Debug, Serialize)]
struct SkillListResponse {
    skills: Vec<SkillRecord>,
}

#[derive(Debug, Serialize)]
struct SkillMutationResponse {
    skill_id: String,
    status: String,
}

#[derive(Debug, Serialize)]
struct SkillStoreResponse {
    items: Vec<Value>,
}

#[derive(Debug, Deserialize)]
struct SkillStoreInstallRequest {
    item_id: String,
}

#[derive(Debug, Clone)]
struct SkillStoreCatalogItem {
    item_id: &'static str,
    name: &'static str,
    description: &'static str,
    path: &'static str,
}

#[derive(Debug, Deserialize)]
struct SandboxToolExecuteRequest {
    session_id: String,
    task_id: Option<String>,
    operation: SandboxOperationKind,
    path: Option<String>,
    content: Option<String>,
    command: Option<String>,
    #[serde(default)]
    args: Vec<String>,
    timeout_ms: Option<u64>,
}

#[derive(Debug, Serialize)]
struct SandboxToolExecuteResponse {
    status: String,
    approval_required: bool,
    approval_id: Option<String>,
    operation: String,
    result: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct SandboxApprovalPath {
    approval_id: String,
}

#[derive(Debug, Deserialize, Default)]
struct SandboxApprovalDecisionRequest {
    actor: Option<String>,
    note: Option<String>,
}

#[derive(Debug, Serialize)]
struct SandboxApprovalDecisionResponse {
    approval_id: String,
    status: String,
    result: Option<Value>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct SandboxApprovalQuery {
    session_id: Option<String>,
    task_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct SandboxApprovalsResponse {
    approvals: Vec<SandboxApprovalRecord>,
}

#[derive(Debug, Serialize)]
struct RuntimeProvidersResponse {
    providers: Vec<runtime_config::ProviderConfig>,
}

#[derive(Debug, Deserialize)]
struct RuntimeProviderRequest {
    name: String,
    kind: String,
    model: String,
    base_url: Option<String>,
    api_key: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RuntimeProviderPath {
    provider_id: i64,
}

#[derive(Debug, Serialize)]
struct RuntimeProviderMutationResponse {
    status: String,
    provider_id: i64,
}

#[derive(Debug, Serialize)]
struct RuntimeLocalClientsResponse {
    clients: Vec<local_client::LocalClientStatus>,
}

#[derive(Debug, Serialize)]
struct RuntimeLocalClientProcessesResponse {
    processes: Vec<local_client::LocalDispatchProcessSnapshot>,
    timestamp_ms: u64,
}

#[derive(Debug, Deserialize)]
struct RuntimeSshTargetRequest {
    name: String,
    host: String,
    port: Option<u16>,
    username: Option<String>,
    identity_file: Option<String>,
    remote_workdir: Option<String>,
    #[serde(default)]
    options: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct RuntimeSshTargetPath {
    ssh_target_id: String,
}

#[derive(Debug, Serialize)]
struct RuntimeSshTargetsResponse {
    targets: Vec<SshTargetRecord>,
}

#[derive(Debug, Serialize)]
struct RuntimeSshTargetMutationResponse {
    status: String,
    ssh_target_id: String,
}

#[derive(Debug, Deserialize)]
struct RuntimeDispatcherQuery {
    project_id: String,
}

#[derive(Debug, Deserialize)]
struct RuntimeSetDispatcherRequest {
    project_id: String,
    target_kind: String,
    target_ref: String,
}

#[derive(Debug, Serialize)]
struct RuntimeDispatcherResponse {
    dispatcher: Option<runtime_config::ProjectDispatcherConfig>,
}

#[derive(Debug, Deserialize)]
struct WorkspaceEnsureFolderRequest {
    path: String,
}

#[derive(Debug, Serialize)]
struct WorkspaceEnsureFolderResponse {
    path: String,
    created: bool,
    existed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkspaceFolderEnsureOutcome {
    Created,
    Existing,
    Skipped,
}

pub async fn serve(addr: &str) -> Result<(), String> {
    let app = build_router(AppState::default());

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| format!("failed to bind {addr}: {e}"))?;
    println!("serve: listening on http://{addr}");
    axum::serve(listener, app)
        .await
        .map_err(|e| format!("server runtime failed: {e}"))?;
    Ok(())
}

fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/kernel/session/open", post(open_session))
        .route("/kernel/task/submit", post(submit_task))
        .route("/kernel/task/abort", post(abort_task))
        .route("/kernel/task/{task_id}/status", get(task_status))
        .route("/kernel/task/{task_id}/events", get(task_events))
        .route("/kernel/deploy", post(deploy))
        .route("/kernel/ws", get(ws_events))
        .route(
            "/project/{project_id}/session/{session_id}/file/status",
            get(file_status),
        )
        .route(
            "/project/{project_id}/session/{session_id}/file",
            get(file_view),
        )
        .route(
            "/project/{project_id}/session/{session_id}/changes/summary",
            get(changes_summary),
        )
        .route(
            "/project/{project_id}/session/{session_id}/changes/ack",
            post(changes_ack),
        )
        .route("/kernel/runtime/providers", get(runtime_list_providers))
        .route("/kernel/runtime/providers", post(runtime_add_provider))
        .route(
            "/kernel/runtime/providers/{provider_id}",
            post(runtime_update_provider),
        )
        .route(
            "/kernel/runtime/providers/{provider_id}",
            axum::routing::delete(runtime_delete_provider),
        )
        .route(
            "/kernel/runtime/local-clients",
            get(runtime_list_local_clients),
        )
        .route(
            "/kernel/runtime/local-clients/processes",
            get(runtime_list_local_client_processes),
        )
        .route(
            "/kernel/runtime/local-clients/dispatch",
            post(runtime_dispatch_local_client),
        )
        .route("/kernel/runtime/ssh-targets", get(runtime_list_ssh_targets))
        .route("/kernel/runtime/ssh-targets", post(runtime_add_ssh_target))
        .route(
            "/kernel/runtime/ssh-targets/{ssh_target_id}",
            post(runtime_update_ssh_target),
        )
        .route(
            "/kernel/runtime/ssh-targets/{ssh_target_id}",
            axum::routing::delete(runtime_delete_ssh_target),
        )
        .route("/kernel/runtime/dispatcher", get(runtime_get_dispatcher))
        .route("/kernel/runtime/dispatcher", post(runtime_set_dispatcher))
        .route("/kernel/skills", get(list_skills))
        .route("/kernel/skills", post(create_skill))
        .route("/kernel/skills/store", get(skill_store))
        .route(
            "/kernel/skills/store/install",
            post(install_skill_store_item),
        )
        .route("/kernel/skills/{skill_id}/activate", post(activate_skill))
        .route(
            "/kernel/skills/{skill_id}/deactivate",
            post(deactivate_skill),
        )
        .route("/kernel/sandbox/tools/execute", post(sandbox_execute_tool))
        .route("/kernel/sandbox/approvals", get(sandbox_list_approvals))
        .route(
            "/kernel/sandbox/approvals/{approval_id}/approve",
            post(sandbox_approve),
        )
        .route(
            "/kernel/sandbox/approvals/{approval_id}/reject",
            post(sandbox_reject),
        )
        .route(
            "/kernel/workspace/ensure-folder",
            post(workspace_ensure_folder),
        )
        .with_state(state)
}

pub(crate) async fn invoke_http_like(
    state: impl Into<Option<AppState>>,
    method: &str,
    path: &str,
    body: Option<Value>,
) -> Result<Value, String> {
    let state = state.into().unwrap_or_default();
    let router = build_router(state);

    let method = Method::from_bytes(method.as_bytes())
        .map_err(|e| format!("invalid method {method}: {e}"))?;
    let body_preview = body
        .as_ref()
        .map(|payload| preview_value(payload, 300))
        .unwrap_or_else(|| "null".to_string());
    eprintln!(
        "kernel.invoke: request method={} path={} body={}",
        method, path, body_preview
    );
    let request_body = match body {
        Some(v) => {
            serde_json::to_vec(&v).map_err(|e| format!("encode request body failed: {e}"))?
        }
        None => vec![],
    };
    let request = Request::builder()
        .method(method.clone())
        .uri(path)
        .header("content-type", "application/json")
        .body(Body::from(request_body))
        .map_err(|e| format!("build request failed: {e}"))?;

    let response = router
        .oneshot(request)
        .await
        .map_err(|e| format!("router dispatch failed: {e}"))?;
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .map_err(|e| format!("read response body failed: {e}"))?;
    let payload: Value = if body.is_empty() {
        json!({})
    } else {
        serde_json::from_slice(&body).map_err(|e| format!("decode response body failed: {e}"))?
    };
    eprintln!(
        "kernel.invoke: response method={} path={} status={} body={}",
        method,
        path,
        status.as_u16(),
        preview_value(&payload, 300)
    );

    if status.is_success() {
        Ok(payload)
    } else {
        let message = payload
            .get("error")
            .and_then(Value::as_str)
            .map(ToString::to_string)
            .unwrap_or_else(|| format!("request failed: {}", status.as_u16()));
        Err(message)
    }
}

async fn open_session(
    State(state): State<AppState>,
    Json(req): Json<OpenSessionRequest>,
) -> ApiResult<OpenSessionResponse> {
    if req.project_id.trim().is_empty() {
        return Err(ApiError::bad_request("project_id must not be empty"));
    }
    let requested_dispatcher = normalize_dispatcher_input(
        req.dispatcher_kind.as_deref(),
        req.dispatcher_ref.as_deref(),
    )?;
    let config_db = runtime_config::default_db_path();
    let persisted_dispatcher = runtime_config::get_project_dispatcher(&config_db, &req.project_id)
        .map_err(ApiError::internal)?;
    let (dispatcher_kind, dispatcher_ref, dispatcher_source) =
        if let Some((kind, reference)) = requested_dispatcher {
            runtime_config::set_project_dispatcher(
                &config_db,
                &NewProjectDispatcher {
                    project_id: req.project_id.clone(),
                    target_kind: kind.clone(),
                    target_ref: reference.clone(),
                },
            )
            .map_err(ApiError::internal)?;
            (kind, reference, "request")
        } else if let Some(saved) = persisted_dispatcher {
            (saved.target_kind, saved.target_ref, "persisted")
        } else {
            default_dispatcher_selection(&config_db)
        };

    let mut guard = state
        .inner
        .lock()
        .map_err(|_| ApiError::internal("session state mutex poisoned"))?;
    guard.next_session_id += 1;
    let session_id = format!("sess_{:04}", guard.next_session_id);
    let target = req
        .target
        .unwrap_or_else(|| "unspecified target".to_string());
    let ensured_target = ensure_workspace_target_dir(&target).map_err(ApiError::internal)?;
    match ensured_target {
        WorkspaceFolderEnsureOutcome::Created => {
            eprintln!(
                "kernel.session.open: created workspace target folder path={}",
                target
            );
        }
        WorkspaceFolderEnsureOutcome::Existing => {
            eprintln!(
                "kernel.session.open: workspace target folder already exists path={}",
                target
            );
        }
        WorkspaceFolderEnsureOutcome::Skipped => {
            eprintln!(
                "kernel.session.open: skipped target folder creation for non-path target={}",
                preview_text(&target, 120)
            );
        }
    }

    guard.sessions.insert(
        session_id.clone(),
        SessionRecord {
            project_id: req.project_id.clone(),
            target: target.clone(),
            dispatcher_kind: Some(dispatcher_kind.clone()),
            dispatcher_ref: Some(dispatcher_ref.clone()),
        },
    );

    let key = session_key(&req.project_id, &session_id);
    guard
        .session_changes
        .entry(key)
        .or_insert_with(|| bootstrap_changes(&session_id));
    drop(guard);
    state.persist();

    eprintln!(
        "kernel.session.open: project_id={} session_id={} target={} dispatcher={}/{} source={}",
        req.project_id, session_id, target, dispatcher_kind, dispatcher_ref, dispatcher_source
    );

    Ok(Json(OpenSessionResponse {
        session_id,
        project_id: req.project_id,
        target,
        dispatcher_kind,
        dispatcher_ref,
        status: "running".to_string(),
    }))
}

async fn submit_task(
    State(state): State<AppState>,
    Json(req): Json<SubmitTaskRequest>,
) -> ApiResult<SubmitTaskResponse> {
    if req.session_id.trim().is_empty() || req.title.trim().is_empty() {
        return Err(ApiError::bad_request(
            "session_id and title must not be empty",
        ));
    }

    let title = req.title.trim().to_string();
    let input = req.input.clone();
    let fallback_dispatcher = default_dispatcher_selection(&runtime_config::default_db_path());
    let requested_dispatcher = normalize_dispatcher_input(
        req.dispatcher_kind.as_deref(),
        req.dispatcher_ref.as_deref(),
    )?;

    let (task_id, session, progress_event, dispatcher_kind, dispatcher_ref, dispatcher_source) = {
        let mut guard = state
            .inner
            .lock()
            .map_err(|_| ApiError::internal("task state mutex poisoned"))?;
        let session = guard
            .sessions
            .get(&req.session_id)
            .cloned()
            .ok_or_else(|| ApiError::not_found(format!("session not found: {}", req.session_id)))?;

        guard.next_task_id += 1;
        let task_id = format!("task_{:04}", guard.next_task_id);
        guard.tasks.insert(
            task_id.clone(),
            TaskRecord {
                task_id: task_id.clone(),
                session_id: req.session_id.clone(),
                title: title.clone(),
                status: "running".to_string(),
            },
        );

        let (dispatcher_kind, dispatcher_ref, dispatcher_source) = if let Some((kind, reference)) =
            requested_dispatcher.clone()
        {
            (kind, reference, "task_override")
        } else {
            let kind = session
                .dispatcher_kind
                .clone()
                .unwrap_or_else(|| fallback_dispatcher.0.clone());
            let reference = session
                .dispatcher_ref
                .clone()
                .unwrap_or_else(|| fallback_dispatcher.1.clone());
            let source = if session.dispatcher_kind.is_some() && session.dispatcher_ref.is_some() {
                "session"
            } else {
                fallback_dispatcher.2
            };
            (kind, reference, source)
        };
        let progress = append_event(
            &mut guard,
            &task_id,
            "task.progress",
            &req.session_id,
            json!({
                "summary": "task accepted and started",
                "title": title,
                "input": input.clone(),
                "session_target": session.target.clone(),
                "dispatcher_kind": dispatcher_kind.clone(),
                "dispatcher_ref": dispatcher_ref.clone(),
                "dispatcher_source": dispatcher_source,
            }),
        );
        (
            task_id,
            session,
            progress,
            dispatcher_kind,
            dispatcher_ref,
            dispatcher_source,
        )
    };
    eprintln!(
        "kernel.task.submit: accepted task_id={} session_id={} project_id={} dispatcher={}/{} source={} title={}",
        task_id,
        req.session_id,
        session.project_id,
        dispatcher_kind,
        dispatcher_ref,
        dispatcher_source,
        preview_text(&title, 120)
    );
    state.publish_event(RealtimeEvent::from_task_event(&progress_event));
    state.persist();

    let submit_state = state.clone();
    let submit_task_id = task_id.clone();
    let submit_session_id = req.session_id.clone();
    let submit_title = title.clone();
    let submit_input = input.clone();
    eprintln!(
        "kernel.task.submit: queued task_id={} session_id={} dispatcher={}/{}",
        submit_task_id, submit_session_id, dispatcher_kind, dispatcher_ref
    );

    tokio::spawn(async move {
        run_task_pipeline_in_background(
            submit_state,
            submit_task_id,
            submit_session_id,
            session,
            submit_title,
            submit_input,
            dispatcher_kind,
            dispatcher_ref,
        )
        .await;
    });

    Ok(Json(SubmitTaskResponse {
        task_id,
        status: "accepted".to_string(),
    }))
}

async fn run_task_pipeline_in_background(
    state: AppState,
    task_id: String,
    session_id: String,
    session: SessionRecord,
    title: String,
    input: Option<String>,
    dispatcher_kind: String,
    dispatcher_ref: String,
) {
    eprintln!(
        "kernel.task.worker: start task_id={} session_id={} dispatcher={}/{} target={}",
        task_id, session_id, dispatcher_kind, dispatcher_ref, session.target
    );
    if let Ok(mut guard) = state.inner.lock() {
        let running = append_event(
            &mut guard,
            &task_id,
            "task.progress",
            &session_id,
            json!({
                "summary": "role pipeline is running",
                "dispatcher_kind": dispatcher_kind.clone(),
                "dispatcher_ref": dispatcher_ref.clone(),
                "session_target": session.target.clone(),
            }),
        );
        state.publish_event(RealtimeEvent::from_task_event(&running));
        drop(guard);
        state.persist();
    } else {
        eprintln!(
            "kernel.task.worker: progress publish skipped task_id={} reason=task state mutex poisoned",
            task_id
        );
    }

    let pipeline_result = run_role_pipeline_for_task(
        state.clone(),
        &task_id,
        &session_id,
        &session,
        title.as_str(),
        input.as_deref(),
        &dispatcher_kind,
        &dispatcher_ref,
    )
    .await;
    eprintln!(
        "kernel.task.worker: pipeline finished task_id={} result={}",
        task_id,
        preview_pipeline_result(&pipeline_result)
    );

    let mut guard = match state.inner.lock() {
        Ok(guard) => guard,
        Err(_) => {
            eprintln!(
                "kernel.task.worker: failed to finalize task_id={} reason=task state mutex poisoned",
                task_id
            );
            return;
        }
    };

    let task_exists = guard.tasks.contains_key(&task_id);
    if !task_exists {
        eprintln!(
            "kernel.task.worker: skip finalize task_id={} reason=task missing",
            task_id
        );
        return;
    }
    let task_aborted = guard
        .tasks
        .get(&task_id)
        .map(|task| task.status == "aborted")
        .unwrap_or(false);
    if task_aborted {
        eprintln!(
            "kernel.task.worker: skip finalize task_id={} reason=task already aborted",
            task_id
        );
        return;
    }

    let key = session_key(&session.project_id, &session_id);
    let (event_type, payload, final_status, selected_agent_id, skill_directive_source) =
        match pipeline_result {
            Ok(result) if result.status == "done" => {
                let selected_agent = result
                    .stages
                    .iter()
                    .rev()
                    .find(|stage| stage.stage_kind == "worker_dispatch" && stage.ok)
                    .map(|stage| stage.role_id.clone())
                    .unwrap_or_else(|| dispatcher_ref.clone());
                let skill_directive_source = result.final_output.clone().or_else(|| {
                    result
                        .stages
                        .iter()
                        .rev()
                        .find_map(|stage| stage.output.clone())
                });
                (
                    "task.done",
                    json!({
                        "summary": "role pipeline completed",
                        "dispatcher_kind": dispatcher_kind.clone(),
                        "dispatcher_ref": dispatcher_ref.clone(),
                        "pipeline_result": result,
                    }),
                    "done",
                    selected_agent,
                    skill_directive_source,
                )
            }
            Ok(result) => (
                "task.error",
                json!({
                    "reason": result
                        .error
                        .clone()
                        .unwrap_or_else(|| "role pipeline execution failed".to_string()),
                    "dispatcher_kind": dispatcher_kind.clone(),
                    "dispatcher_ref": dispatcher_ref.clone(),
                    "pipeline_result": result,
                }),
                "error",
                dispatcher_ref.clone(),
                None,
            ),
            Err(err) => (
                "task.error",
                json!({
                    "reason": err,
                    "dispatcher_kind": dispatcher_kind.clone(),
                    "dispatcher_ref": dispatcher_ref.clone(),
                }),
                "error",
                dispatcher_ref.clone(),
                None,
            ),
        };

    let done = append_event(&mut guard, &task_id, event_type, &session_id, payload);
    state.publish_event(RealtimeEvent::from_task_event(&done));

    let idle = append_event(
        &mut guard,
        &task_id,
        "task.idle_waiting",
        &session_id,
        json!({
            "summary": "no pending tasks",
        }),
    );
    state.publish_event(RealtimeEvent::from_task_event(&idle));

    if let Some(task) = guard.tasks.get_mut(&task_id) {
        task.status = final_status.to_string();
    }

    if final_status == "done" {
        guard
            .session_changes
            .entry(key)
            .or_default()
            .push(ChangeFile {
                path: format!("src/{}_result.rs", slugify(&task_id)),
                status: ChangeStatus::Modified,
                additions: 8,
                deletions: 2,
                task_id: task_id.clone(),
                agent_id: selected_agent_id.clone(),
            });
    }
    drop(guard);
    state.persist();
    if final_status == "done" {
        if let Some(output) = skill_directive_source.as_deref() {
            apply_ai_skill_directives(&state, &session_id, &task_id, output);
        }
    }
    eprintln!(
        "kernel.task.worker: finalized task_id={} final_status={} selected_agent={}",
        task_id, final_status, selected_agent_id
    );
}

async fn abort_task(
    State(state): State<AppState>,
    Json(req): Json<AbortTaskRequest>,
) -> ApiResult<AbortTaskResponse> {
    let mut guard = state
        .inner
        .lock()
        .map_err(|_| ApiError::internal("task state mutex poisoned"))?;
    let session_id = {
        let task = guard
            .tasks
            .get_mut(&req.task_id)
            .ok_or_else(|| ApiError::not_found(format!("task not found: {}", req.task_id)))?;
        task.status = "aborted".to_string();
        task.session_id.clone()
    };

    let aborted = append_event(
        &mut guard,
        &req.task_id,
        "task.error",
        &session_id,
        json!({
            "reason": req.reason.unwrap_or_else(|| "aborted by operator".to_string())
        }),
    );
    state.publish_event(RealtimeEvent::from_task_event(&aborted));
    drop(guard);
    state.persist();

    Ok(Json(AbortTaskResponse {
        task_id: req.task_id,
        status: "aborted".to_string(),
    }))
}

async fn task_status(
    State(state): State<AppState>,
    Path(task_id): Path<String>,
) -> ApiResult<TaskStatusResponse> {
    let guard = state
        .inner
        .lock()
        .map_err(|_| ApiError::internal("task state mutex poisoned"))?;
    let task = guard
        .tasks
        .get(&task_id)
        .ok_or_else(|| ApiError::not_found(format!("task not found: {task_id}")))?;

    Ok(Json(TaskStatusResponse {
        task_id: task.task_id.clone(),
        session_id: task.session_id.clone(),
        title: task.title.clone(),
        status: task.status.clone(),
    }))
}

async fn task_events(
    State(state): State<AppState>,
    Path(task_id): Path<String>,
) -> ApiResult<TaskEventsResponse> {
    let guard = state
        .inner
        .lock()
        .map_err(|_| ApiError::internal("task state mutex poisoned"))?;
    if !guard.tasks.contains_key(&task_id) {
        return Err(ApiError::not_found(format!("task not found: {task_id}")));
    }
    let events = guard.task_events.get(&task_id).cloned().unwrap_or_default();
    Ok(Json(TaskEventsResponse { task_id, events }))
}

async fn deploy(
    State(state): State<AppState>,
    Json(req): Json<DeployRequest>,
) -> ApiResult<DeployResponse> {
    let deploy_session = req.session_id.clone().unwrap_or_else(|| "n/a".to_string());
    let deploy_project = req.project_id.clone();

    let mut guard = state
        .inner
        .lock()
        .map_err(|_| ApiError::internal("deploy state mutex poisoned"))?;
    guard.next_deploy_id += 1;
    guard.next_event_id += 1;
    let started_event_id = format!("evt_{:05}", guard.next_event_id);
    guard.next_event_id += 1;
    let finished_event_id = format!("evt_{:05}", guard.next_event_id);
    let deploy_id = format!("deploy_{:04}", guard.next_deploy_id);
    drop(guard);
    state.persist();

    state.publish_event(RealtimeEvent {
        event_id: started_event_id,
        event_type: "deploy.started".to_string(),
        session_id: deploy_session.clone(),
        task_id: None,
        timestamp_ms: now_ms(),
        payload: json!({
            "deploy_id": deploy_id,
            "project_id": deploy_project,
        }),
    });
    state.publish_event(RealtimeEvent {
        event_id: finished_event_id,
        event_type: "deploy.finished".to_string(),
        session_id: deploy_session,
        task_id: None,
        timestamp_ms: now_ms(),
        payload: json!({
            "deploy_id": deploy_id,
            "status": "accepted",
        }),
    });

    Ok(Json(DeployResponse {
        deploy_id: deploy_id.clone(),
        status: "accepted".to_string(),
        project_id: req.project_id,
        session_id: req.session_id,
    }))
}

async fn runtime_list_providers() -> ApiResult<RuntimeProvidersResponse> {
    let db_path = runtime_config::default_db_path();
    let providers = runtime_config::list_providers(&db_path).map_err(ApiError::internal)?;
    Ok(Json(RuntimeProvidersResponse { providers }))
}

async fn runtime_add_provider(
    Json(req): Json<RuntimeProviderRequest>,
) -> ApiResult<RuntimeProviderMutationResponse> {
    let db_path = runtime_config::default_db_path();
    let provider_id = runtime_config::add_provider(
        &db_path,
        &NewProvider {
            name: req.name,
            kind: req.kind,
            model: req.model,
            api_key: req.api_key,
            base_url: req.base_url,
        },
    )
    .map_err(ApiError::internal)?;
    Ok(Json(RuntimeProviderMutationResponse {
        status: "created".to_string(),
        provider_id,
    }))
}

async fn runtime_update_provider(
    Path(path): Path<RuntimeProviderPath>,
    Json(req): Json<RuntimeProviderRequest>,
) -> ApiResult<RuntimeProviderMutationResponse> {
    let db_path = runtime_config::default_db_path();
    runtime_config::update_provider(
        &db_path,
        path.provider_id,
        &NewProvider {
            name: req.name,
            kind: req.kind,
            model: req.model,
            api_key: req.api_key,
            base_url: req.base_url,
        },
    )
    .map_err(ApiError::internal)?;
    Ok(Json(RuntimeProviderMutationResponse {
        status: "updated".to_string(),
        provider_id: path.provider_id,
    }))
}

async fn runtime_delete_provider(
    Path(path): Path<RuntimeProviderPath>,
) -> ApiResult<RuntimeProviderMutationResponse> {
    let db_path = runtime_config::default_db_path();
    runtime_config::delete_provider(&db_path, path.provider_id).map_err(ApiError::internal)?;
    Ok(Json(RuntimeProviderMutationResponse {
        status: "deleted".to_string(),
        provider_id: path.provider_id,
    }))
}

async fn runtime_list_local_clients() -> ApiResult<RuntimeLocalClientsResponse> {
    Ok(Json(RuntimeLocalClientsResponse {
        clients: local_client::list_local_clients(),
    }))
}

async fn runtime_list_local_client_processes() -> ApiResult<RuntimeLocalClientProcessesResponse> {
    Ok(Json(RuntimeLocalClientProcessesResponse {
        processes: local_client::list_active_dispatch_processes(),
        timestamp_ms: now_ms(),
    }))
}

async fn runtime_dispatch_local_client(
    Json(req): Json<local_client::LocalDispatchRequest>,
) -> ApiResult<local_client::LocalDispatchResult> {
    let response = local_client::dispatch_prompt(&req)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(response))
}

async fn runtime_list_ssh_targets(
    State(state): State<AppState>,
) -> ApiResult<RuntimeSshTargetsResponse> {
    let guard = state
        .inner
        .lock()
        .map_err(|_| ApiError::internal("ssh target state mutex poisoned"))?;
    let mut targets = guard.ssh_targets.values().cloned().collect::<Vec<_>>();
    targets.sort_by(|left, right| left.ssh_target_id.cmp(&right.ssh_target_id));
    Ok(Json(RuntimeSshTargetsResponse { targets }))
}

async fn runtime_add_ssh_target(
    State(state): State<AppState>,
    Json(req): Json<RuntimeSshTargetRequest>,
) -> ApiResult<RuntimeSshTargetMutationResponse> {
    let normalized = normalize_ssh_target_request(req)?;
    let now = now_ms();
    let ssh_target_id = {
        let mut guard = state
            .inner
            .lock()
            .map_err(|_| ApiError::internal("ssh target state mutex poisoned"))?;
        guard.next_ssh_target_id += 1;
        let ssh_target_id = format!("ssh_{:04}", guard.next_ssh_target_id);
        guard.ssh_targets.insert(
            ssh_target_id.clone(),
            SshTargetRecord {
                ssh_target_id: ssh_target_id.clone(),
                name: normalized.name,
                host: normalized.host,
                port: normalized.port,
                username: normalized.username,
                identity_file: normalized.identity_file,
                remote_workdir: normalized.remote_workdir,
                options: normalized.options,
                created_at_ms: now,
                updated_at_ms: now,
            },
        );
        ssh_target_id
    };

    state.persist();
    Ok(Json(RuntimeSshTargetMutationResponse {
        status: "created".to_string(),
        ssh_target_id,
    }))
}

async fn runtime_update_ssh_target(
    State(state): State<AppState>,
    Path(path): Path<RuntimeSshTargetPath>,
    Json(req): Json<RuntimeSshTargetRequest>,
) -> ApiResult<RuntimeSshTargetMutationResponse> {
    let normalized = normalize_ssh_target_request(req)?;
    let mut guard = state
        .inner
        .lock()
        .map_err(|_| ApiError::internal("ssh target state mutex poisoned"))?;
    let record = guard
        .ssh_targets
        .get_mut(&path.ssh_target_id)
        .ok_or_else(|| {
            ApiError::not_found(format!("ssh target not found: {}", path.ssh_target_id))
        })?;
    record.name = normalized.name;
    record.host = normalized.host;
    record.port = normalized.port;
    record.username = normalized.username;
    record.identity_file = normalized.identity_file;
    record.remote_workdir = normalized.remote_workdir;
    record.options = normalized.options;
    record.updated_at_ms = now_ms();
    let ssh_target_id = record.ssh_target_id.clone();
    drop(guard);

    state.persist();
    Ok(Json(RuntimeSshTargetMutationResponse {
        status: "updated".to_string(),
        ssh_target_id,
    }))
}

async fn runtime_delete_ssh_target(
    State(state): State<AppState>,
    Path(path): Path<RuntimeSshTargetPath>,
) -> ApiResult<RuntimeSshTargetMutationResponse> {
    let removed = {
        let mut guard = state
            .inner
            .lock()
            .map_err(|_| ApiError::internal("ssh target state mutex poisoned"))?;
        guard.ssh_targets.remove(&path.ssh_target_id)
    };

    if removed.is_none() {
        return Err(ApiError::not_found(format!(
            "ssh target not found: {}",
            path.ssh_target_id
        )));
    }

    state.persist();
    Ok(Json(RuntimeSshTargetMutationResponse {
        status: "deleted".to_string(),
        ssh_target_id: path.ssh_target_id,
    }))
}

struct NormalizedSshTargetRequest {
    name: String,
    host: String,
    port: Option<u16>,
    username: Option<String>,
    identity_file: Option<String>,
    remote_workdir: Option<String>,
    options: Vec<String>,
}

fn normalize_ssh_target_request(
    req: RuntimeSshTargetRequest,
) -> Result<NormalizedSshTargetRequest, ApiError> {
    let name = req.name.trim();
    if name.is_empty() {
        return Err(ApiError::bad_request("ssh target name must not be empty"));
    }
    let host = req.host.trim();
    if host.is_empty() {
        return Err(ApiError::bad_request("ssh target host must not be empty"));
    }

    Ok(NormalizedSshTargetRequest {
        name: name.to_string(),
        host: host.to_string(),
        port: req.port,
        username: req
            .username
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string),
        identity_file: req
            .identity_file
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string),
        remote_workdir: req
            .remote_workdir
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string),
        options: req
            .options
            .into_iter()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .collect(),
    })
}

async fn runtime_get_dispatcher(
    Query(query): Query<RuntimeDispatcherQuery>,
) -> ApiResult<RuntimeDispatcherResponse> {
    if query.project_id.trim().is_empty() {
        return Err(ApiError::bad_request("project_id must not be empty"));
    }
    let db_path = runtime_config::default_db_path();
    let dispatcher = runtime_config::get_project_dispatcher(&db_path, &query.project_id)
        .map_err(ApiError::internal)?;
    Ok(Json(RuntimeDispatcherResponse { dispatcher }))
}

async fn runtime_set_dispatcher(
    Json(req): Json<RuntimeSetDispatcherRequest>,
) -> ApiResult<RuntimeDispatcherResponse> {
    if req.project_id.trim().is_empty() {
        return Err(ApiError::bad_request("project_id must not be empty"));
    }

    normalize_dispatcher_input(Some(&req.target_kind), Some(&req.target_ref))?;

    let db_path = runtime_config::default_db_path();
    runtime_config::set_project_dispatcher(
        &db_path,
        &NewProjectDispatcher {
            project_id: req.project_id.clone(),
            target_kind: req.target_kind,
            target_ref: req.target_ref,
        },
    )
    .map_err(ApiError::internal)?;

    let dispatcher = runtime_config::get_project_dispatcher(&db_path, &req.project_id)
        .map_err(ApiError::internal)?;
    Ok(Json(RuntimeDispatcherResponse { dispatcher }))
}

async fn list_skills(State(state): State<AppState>) -> ApiResult<SkillListResponse> {
    let guard = state
        .inner
        .lock()
        .map_err(|_| ApiError::internal("skills state mutex poisoned"))?;
    let mut skills = guard.skills.values().cloned().collect::<Vec<_>>();
    skills.sort_by(|left, right| left.skill_id.cmp(&right.skill_id));
    Ok(Json(SkillListResponse { skills }))
}

async fn create_skill(
    State(state): State<AppState>,
    Json(req): Json<SkillCreateRequest>,
) -> ApiResult<SkillMutationResponse> {
    let name = req.name.trim();
    if name.is_empty() {
        return Err(ApiError::bad_request("skill name must not be empty"));
    }

    let path = req
        .path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| format!("skills/{}/SKILL.md", slugify(name)));
    let description = req
        .description
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("")
        .to_string();

    let now = now_ms();
    let skill_id = {
        let mut guard = state
            .inner
            .lock()
            .map_err(|_| ApiError::internal("skills state mutex poisoned"))?;
        guard.next_skill_id += 1;
        let skill_id = format!("skill_{:04}", guard.next_skill_id);
        guard.skills.insert(
            skill_id.clone(),
            SkillRecord {
                skill_id: skill_id.clone(),
                name: name.to_string(),
                description,
                path,
                active: false,
                created_at_ms: now,
                updated_at_ms: now,
            },
        );
        skill_id
    };

    state.persist();
    Ok(Json(SkillMutationResponse {
        skill_id,
        status: "created".to_string(),
    }))
}

async fn activate_skill(
    State(state): State<AppState>,
    Path(path): Path<SkillPath>,
) -> ApiResult<SkillMutationResponse> {
    let mut guard = state
        .inner
        .lock()
        .map_err(|_| ApiError::internal("skills state mutex poisoned"))?;
    let skill = guard
        .skills
        .get_mut(&path.skill_id)
        .ok_or_else(|| ApiError::not_found(format!("skill not found: {}", path.skill_id)))?;
    skill.active = true;
    skill.updated_at_ms = now_ms();
    let skill_id = skill.skill_id.clone();
    drop(guard);
    state.persist();

    Ok(Json(SkillMutationResponse {
        skill_id,
        status: "activated".to_string(),
    }))
}

async fn deactivate_skill(
    State(state): State<AppState>,
    Path(path): Path<SkillPath>,
) -> ApiResult<SkillMutationResponse> {
    let mut guard = state
        .inner
        .lock()
        .map_err(|_| ApiError::internal("skills state mutex poisoned"))?;
    let skill = guard
        .skills
        .get_mut(&path.skill_id)
        .ok_or_else(|| ApiError::not_found(format!("skill not found: {}", path.skill_id)))?;
    skill.active = false;
    skill.updated_at_ms = now_ms();
    let skill_id = skill.skill_id.clone();
    drop(guard);
    state.persist();

    Ok(Json(SkillMutationResponse {
        skill_id,
        status: "deactivated".to_string(),
    }))
}

async fn skill_store(State(state): State<AppState>) -> ApiResult<SkillStoreResponse> {
    let guard = state
        .inner
        .lock()
        .map_err(|_| ApiError::internal("skills state mutex poisoned"))?;
    let catalog = skill_store_catalog();

    let items = catalog
        .iter()
        .map(|item| {
            let installed = find_skill_for_catalog_item(item, &guard.skills);
            json!({
                "item_id": item.item_id,
                "name": item.name,
                "description": item.description,
                "path": item.path,
                "installed": installed.is_some(),
                "active": installed.as_ref().map(|skill| skill.active).unwrap_or(false),
                "installed_skill_id": installed.as_ref().map(|skill| skill.skill_id.clone()),
            })
        })
        .collect::<Vec<_>>();
    Ok(Json(SkillStoreResponse { items }))
}

async fn install_skill_store_item(
    State(state): State<AppState>,
    Json(req): Json<SkillStoreInstallRequest>,
) -> ApiResult<SkillMutationResponse> {
    let item_id = req.item_id.trim();
    if item_id.is_empty() {
        return Err(ApiError::bad_request("item_id must not be empty"));
    }

    let item = skill_store_catalog()
        .into_iter()
        .find(|entry| entry.item_id == item_id)
        .ok_or_else(|| ApiError::not_found(format!("skill store item not found: {item_id}")))?;

    let now = now_ms();
    let (skill_id, status) = {
        let mut guard = state
            .inner
            .lock()
            .map_err(|_| ApiError::internal("skills state mutex poisoned"))?;
        if let Some(existing) = find_skill_for_catalog_item(&item, &guard.skills) {
            (existing.skill_id.clone(), "already_installed".to_string())
        } else {
            guard.next_skill_id += 1;
            let skill_id = format!("skill_{:04}", guard.next_skill_id);
            guard.skills.insert(
                skill_id.clone(),
                SkillRecord {
                    skill_id: skill_id.clone(),
                    name: item.name.to_string(),
                    description: item.description.to_string(),
                    path: item.path.to_string(),
                    active: false,
                    created_at_ms: now,
                    updated_at_ms: now,
                },
            );
            (skill_id, "installed".to_string())
        }
    };

    state.persist();
    Ok(Json(SkillMutationResponse { skill_id, status }))
}

async fn sandbox_list_approvals(
    State(state): State<AppState>,
    Query(query): Query<SandboxApprovalQuery>,
) -> ApiResult<SandboxApprovalsResponse> {
    let guard = state
        .inner
        .lock()
        .map_err(|_| ApiError::internal("sandbox approval state mutex poisoned"))?;

    let mut approvals = guard
        .pending_approvals
        .values()
        .filter(|record| {
            let session_ok = query
                .session_id
                .as_deref()
                .map(|session_id| session_id == record.session_id)
                .unwrap_or(true);
            let task_ok = query
                .task_id
                .as_deref()
                .map(|task_id| record.task_id.as_deref() == Some(task_id))
                .unwrap_or(true);
            session_ok && task_ok
        })
        .cloned()
        .collect::<Vec<_>>();
    approvals.sort_by(|left, right| left.approval_id.cmp(&right.approval_id));

    Ok(Json(SandboxApprovalsResponse { approvals }))
}

async fn sandbox_execute_tool(
    State(state): State<AppState>,
    Json(req): Json<SandboxToolExecuteRequest>,
) -> ApiResult<SandboxToolExecuteResponse> {
    let session_id = req.session_id.trim();
    if session_id.is_empty() {
        return Err(ApiError::bad_request("session_id must not be empty"));
    }
    let task_id = req
        .task_id
        .as_deref()
        .map(str::trim)
        .filter(|task_id| !task_id.is_empty())
        .map(ToString::to_string);

    let session = load_session_record(&state, session_id)?;
    let action = SandboxAction {
        operation: req.operation,
        path: req.path,
        content: req.content,
        command: req.command,
        args: req.args,
        timeout_ms: req.timeout_ms,
    };
    let policy = load_sandbox_runtime_policy();

    if matches!(policy.approval_mode, SandboxApprovalMode::AskFirst) {
        let reason = "ask-first policy requires approval before sandbox operation";
        let approval_id = queue_sandbox_approval(
            &state,
            session_id,
            task_id.as_deref(),
            action.clone(),
            reason.to_string(),
        )?;
        publish_kernel_event(
            &state,
            session_id,
            task_id.as_deref(),
            "tool.approval_required",
            json!({
                "summary": "sandbox operation requires approval",
                "approval_id": approval_id,
                "reason": reason,
                "action": sandbox_action_preview(&action),
            }),
        );
        return Ok(Json(SandboxToolExecuteResponse {
            status: "approval_required".to_string(),
            approval_required: true,
            approval_id: Some(approval_id),
            operation: action.operation.as_str().to_string(),
            result: None,
        }));
    }

    let result = execute_sandbox_action(&session, &action, &policy).await;
    match result {
        Ok(output) => {
            publish_kernel_event(
                &state,
                session_id,
                task_id.as_deref(),
                "tool.executed",
                json!({
                    "summary": "sandbox operation executed",
                    "action": sandbox_action_preview(&action),
                    "result": output,
                }),
            );
            Ok(Json(SandboxToolExecuteResponse {
                status: "executed".to_string(),
                approval_required: false,
                approval_id: None,
                operation: action.operation.as_str().to_string(),
                result: Some(output),
            }))
        }
        Err(err) => {
            publish_kernel_event(
                &state,
                session_id,
                task_id.as_deref(),
                "tool.execution_failed",
                json!({
                    "summary": "sandbox operation failed",
                    "action": sandbox_action_preview(&action),
                    "reason": err,
                }),
            );
            Err(ApiError::bad_request(err))
        }
    }
}

async fn sandbox_approve(
    State(state): State<AppState>,
    Path(path): Path<SandboxApprovalPath>,
    Json(req): Json<SandboxApprovalDecisionRequest>,
) -> ApiResult<SandboxApprovalDecisionResponse> {
    let actor = req
        .actor
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("operator")
        .to_string();
    let approval = take_pending_approval(&state, &path.approval_id)?;

    publish_kernel_event(
        &state,
        &approval.session_id,
        approval.task_id.as_deref(),
        "tool.approval_granted",
        json!({
            "summary": "sandbox approval granted",
            "approval_id": approval.approval_id,
            "actor": actor,
            "note": req.note,
            "action": sandbox_action_preview(&approval.action),
        }),
    );

    let session = load_session_record(&state, &approval.session_id)?;
    let policy = load_sandbox_runtime_policy();
    match execute_sandbox_action(&session, &approval.action, &policy).await {
        Ok(output) => {
            publish_kernel_event(
                &state,
                &approval.session_id,
                approval.task_id.as_deref(),
                "tool.executed",
                json!({
                    "summary": "approved sandbox operation executed",
                    "approval_id": approval.approval_id,
                    "actor": actor,
                    "action": sandbox_action_preview(&approval.action),
                    "result": output,
                }),
            );
            Ok(Json(SandboxApprovalDecisionResponse {
                approval_id: approval.approval_id,
                status: "executed".to_string(),
                result: Some(output),
            }))
        }
        Err(err) => {
            publish_kernel_event(
                &state,
                &approval.session_id,
                approval.task_id.as_deref(),
                "tool.execution_failed",
                json!({
                    "summary": "approved sandbox operation failed",
                    "approval_id": approval.approval_id,
                    "actor": actor,
                    "action": sandbox_action_preview(&approval.action),
                    "reason": err,
                }),
            );
            Err(ApiError::bad_request(err))
        }
    }
}

async fn sandbox_reject(
    State(state): State<AppState>,
    Path(path): Path<SandboxApprovalPath>,
    Json(req): Json<SandboxApprovalDecisionRequest>,
) -> ApiResult<SandboxApprovalDecisionResponse> {
    let actor = req
        .actor
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("operator")
        .to_string();
    let approval = take_pending_approval(&state, &path.approval_id)?;

    publish_kernel_event(
        &state,
        &approval.session_id,
        approval.task_id.as_deref(),
        "tool.approval_rejected",
        json!({
            "summary": "sandbox approval rejected",
            "approval_id": approval.approval_id,
            "actor": actor,
            "note": req.note,
            "action": sandbox_action_preview(&approval.action),
        }),
    );

    Ok(Json(SandboxApprovalDecisionResponse {
        approval_id: approval.approval_id,
        status: "rejected".to_string(),
        result: None,
    }))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SandboxApprovalMode {
    AskFirst,
    Auto,
}

#[derive(Debug, Clone)]
struct SandboxRuntimePolicy {
    approval_mode: SandboxApprovalMode,
    allowed_commands: HashSet<String>,
}

fn load_sandbox_runtime_policy() -> SandboxRuntimePolicy {
    let mut allowed_commands = [
        "git", "cargo", "npm", "pnpm", "ls", "cat", "rg", "sed", "bash", "sh",
    ]
    .into_iter()
    .map(ToString::to_string)
    .collect::<HashSet<_>>();

    if let Some(spec) = load_policy_spec_for_runtime() {
        let from_policy = spec
            .policy
            .allowed_commands
            .iter()
            .map(|command| command.trim().to_lowercase())
            .filter(|command| !command.is_empty())
            .collect::<HashSet<_>>();
        if !from_policy.is_empty() {
            allowed_commands = from_policy;
        }
    }

    let approval_mode = match std::env::var("SPIRAL_ORGAN_SANDBOX_APPROVAL_MODE")
        .or_else(|_| std::env::var("SPIRAL_SANDBOX_APPROVAL_MODE"))
        .unwrap_or_else(|_| "ask-first".to_string())
        .to_lowercase()
        .as_str()
    {
        "auto" | "allow" | "auto-approve" => SandboxApprovalMode::Auto,
        _ => SandboxApprovalMode::AskFirst,
    };

    SandboxRuntimePolicy {
        approval_mode,
        allowed_commands,
    }
}

fn load_policy_spec_for_runtime() -> Option<crate::validation::PolicySpec> {
    let candidate = std::env::var("SPIRAL_ORGAN_POLICY_PATH")
        .or_else(|_| std::env::var("SPIRAL_POLICY_PATH"))
        .unwrap_or_else(|_| ".design/specs/policy.default.toml".to_string());
    let content = fs::read_to_string(candidate).ok()?;
    crate::validation::parse_policy_toml(&content).ok()
}

fn queue_sandbox_approval(
    state: &AppState,
    session_id: &str,
    task_id: Option<&str>,
    action: SandboxAction,
    reason: String,
) -> Result<String, ApiError> {
    let mut guard = state
        .inner
        .lock()
        .map_err(|_| ApiError::internal("sandbox approval state mutex poisoned"))?;
    guard.next_approval_id += 1;
    let approval_id = format!("apr_{:05}", guard.next_approval_id);
    guard.pending_approvals.insert(
        approval_id.clone(),
        SandboxApprovalRecord {
            approval_id: approval_id.clone(),
            session_id: session_id.to_string(),
            task_id: task_id.map(ToString::to_string),
            reason,
            action,
            created_at_ms: now_ms(),
        },
    );
    drop(guard);
    state.persist();
    Ok(approval_id)
}

fn take_pending_approval(
    state: &AppState,
    approval_id: &str,
) -> Result<SandboxApprovalRecord, ApiError> {
    let mut guard = state
        .inner
        .lock()
        .map_err(|_| ApiError::internal("sandbox approval state mutex poisoned"))?;
    let record = guard
        .pending_approvals
        .remove(approval_id)
        .ok_or_else(|| ApiError::not_found(format!("approval not found: {approval_id}")))?;
    drop(guard);
    state.persist();
    Ok(record)
}

fn load_session_record(state: &AppState, session_id: &str) -> Result<SessionRecord, ApiError> {
    let guard = state
        .inner
        .lock()
        .map_err(|_| ApiError::internal("session state mutex poisoned"))?;
    guard
        .sessions
        .get(session_id)
        .cloned()
        .ok_or_else(|| ApiError::not_found(format!("session not found: {session_id}")))
}

fn sandbox_action_preview(action: &SandboxAction) -> Value {
    json!({
        "operation": action.operation.as_str(),
        "path": action.path,
        "command": action.command,
        "args": action.args,
        "content_bytes": action.content.as_ref().map(|text| text.len()).unwrap_or(0),
        "timeout_ms": action.timeout_ms
    })
}

fn publish_kernel_event(
    state: &AppState,
    session_id: &str,
    task_id: Option<&str>,
    event_type: &str,
    payload: Value,
) {
    let mut event_opt = None;
    if let Ok(mut guard) = state.inner.lock() {
        if let Some(task_id) = task_id
            .map(str::trim)
            .filter(|task_id| !task_id.is_empty() && guard.tasks.contains_key(*task_id))
        {
            let task_event = append_event(&mut guard, task_id, event_type, session_id, payload);
            event_opt = Some(RealtimeEvent::from_task_event(&task_event));
        } else {
            guard.next_event_id += 1;
            event_opt = Some(RealtimeEvent {
                event_id: format!("evt_{:05}", guard.next_event_id),
                event_type: event_type.to_string(),
                session_id: session_id.to_string(),
                task_id: task_id.map(ToString::to_string),
                timestamp_ms: now_ms(),
                payload,
            });
        }
    } else {
        eprintln!("kernel.event: skipped publish because state mutex is poisoned");
    }

    if let Some(event) = event_opt {
        state.publish_event(event);
        state.persist();
    }
}

async fn execute_sandbox_action(
    session: &SessionRecord,
    action: &SandboxAction,
    policy: &SandboxRuntimePolicy,
) -> Result<Value, String> {
    let workspace_root = resolve_workspace_root(&session.target)?;
    match action.operation {
        SandboxOperationKind::FileWrite => {
            let relative = action
                .path
                .as_deref()
                .ok_or_else(|| "path is required for file_write".to_string())?;
            let absolute = resolve_workspace_relative_path(&workspace_root, relative)?;
            if let Some(parent) = absolute.parent() {
                fs::create_dir_all(parent)
                    .map_err(|err| format!("failed to create parent directory: {err}"))?;
            }
            let content = action.content.clone().unwrap_or_default();
            fs::write(&absolute, content.as_bytes())
                .map_err(|err| format!("failed to write file {}: {err}", absolute.display()))?;
            Ok(json!({
                "path": relative,
                "bytes_written": content.len()
            }))
        }
        SandboxOperationKind::FileDelete => {
            let relative = action
                .path
                .as_deref()
                .ok_or_else(|| "path is required for file_delete".to_string())?;
            let absolute = resolve_workspace_relative_path(&workspace_root, relative)?;
            if !absolute.exists() {
                return Err(format!("file does not exist: {}", absolute.display()));
            }
            if absolute.is_dir() {
                return Err(format!(
                    "path points to a directory, expected file: {}",
                    relative
                ));
            }
            fs::remove_file(&absolute)
                .map_err(|err| format!("failed to delete file {}: {err}", absolute.display()))?;
            Ok(json!({
                "path": relative,
                "deleted": true
            }))
        }
        SandboxOperationKind::DirectoryCreate => {
            let relative = action
                .path
                .as_deref()
                .ok_or_else(|| "path is required for directory_create".to_string())?;
            let absolute = resolve_workspace_relative_path(&workspace_root, relative)?;
            let existed = absolute.exists();
            fs::create_dir_all(&absolute).map_err(|err| {
                format!("failed to create directory {}: {err}", absolute.display())
            })?;
            Ok(json!({
                "path": relative,
                "created": !existed,
                "existed": existed
            }))
        }
        SandboxOperationKind::DirectoryDelete => {
            let relative = action
                .path
                .as_deref()
                .ok_or_else(|| "path is required for directory_delete".to_string())?;
            let absolute = resolve_workspace_relative_path(&workspace_root, relative)?;
            if !absolute.exists() {
                return Err(format!("directory does not exist: {}", absolute.display()));
            }
            if !absolute.is_dir() {
                return Err(format!("path is not a directory: {}", relative));
            }
            fs::remove_dir_all(&absolute).map_err(|err| {
                format!(
                    "failed to delete directory recursively {}: {err}",
                    absolute.display()
                )
            })?;
            Ok(json!({
                "path": relative,
                "deleted": true
            }))
        }
        SandboxOperationKind::CommandRun => {
            let command = action
                .command
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| "command is required for command_run".to_string())?;
            enforce_command_policy(command, &policy.allowed_commands)?;

            let timeout_ms = action.timeout_ms.unwrap_or(120_000).clamp(1_000, 600_000);
            let started = tokio::time::Instant::now();
            let mut process = tokio::process::Command::new(command);
            process
                .args(action.args.iter())
                .current_dir(&workspace_root);

            let output = tokio::time::timeout(
                tokio::time::Duration::from_millis(timeout_ms),
                process.output(),
            )
            .await
            .map_err(|_| format!("command timed out after {timeout_ms} ms"))?
            .map_err(|err| format!("failed to execute command '{command}': {err}"))?;

            Ok(json!({
                "command": command,
                "args": action.args,
                "stdout": String::from_utf8_lossy(&output.stdout).to_string(),
                "stderr": String::from_utf8_lossy(&output.stderr).to_string(),
                "exit_code": output.status.code(),
                "duration_ms": started.elapsed().as_millis() as u64,
                "workspace_root": workspace_root.display().to_string(),
            }))
        }
        SandboxOperationKind::McpCall => {
            let command = action
                .command
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| "command is required for mcp_call".to_string())?;
            enforce_command_policy(command, &policy.allowed_commands)?;

            let method = action
                .path
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    "path is required for mcp_call (use it as jsonrpc method)".to_string()
                })?;
            let raw_params = action.content.clone().unwrap_or_else(|| "{}".to_string());
            let params: Value = serde_json::from_str(&raw_params)
                .map_err(|err| format!("invalid mcp_call params json: {err}"))?;

            let timeout_ms = action.timeout_ms.unwrap_or(120_000).clamp(1_000, 600_000);
            execute_mcp_action(
                &workspace_root,
                command,
                &action.args,
                method,
                params,
                timeout_ms,
            )
            .await
        }
    }
}

async fn execute_mcp_action(
    workspace_root: &FsPath,
    command: &str,
    args: &[String],
    method: &str,
    params: Value,
    timeout_ms: u64,
) -> Result<Value, String> {
    let started = Instant::now();
    let mut process = TokioCommand::new(command);
    process
        .args(args.iter())
        .current_dir(workspace_root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let mut child = process
        .spawn()
        .map_err(|err| format!("failed to spawn mcp server '{command}': {err}"))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| "mcp server stdin is not available".to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "mcp server stdout is not available".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "mcp server stderr is not available".to_string())?;

    let stderr_task = tokio::spawn(async move { read_stream_limited(stderr, 96 * 1024).await });
    let mut lines = AsyncBufReader::new(stdout).lines();

    let init_request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "clientInfo": {
                "name": "spiral_organ_core",
                "version": env!("CARGO_PKG_VERSION"),
            },
            "capabilities": {}
        }
    });
    let initialized_notification = json!({
        "jsonrpc": "2.0",
        "method": "initialized",
        "params": {}
    });
    let call_request = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": method,
        "params": params,
    });

    let protocol = async {
        write_jsonrpc_line(&mut stdin, &init_request).await?;
        let init_response = read_jsonrpc_response(&mut lines, 1).await?;
        write_jsonrpc_line(&mut stdin, &initialized_notification).await?;
        write_jsonrpc_line(&mut stdin, &call_request).await?;
        let call_response = read_jsonrpc_response(&mut lines, 2).await?;
        Ok::<(Value, Value), String>((init_response, call_response))
    };

    let (init_response, call_response) =
        tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), protocol)
            .await
            .map_err(|_| format!("mcp call timed out after {timeout_ms} ms"))??;

    let _ = child.start_kill();
    let _ = tokio::time::timeout(std::time::Duration::from_millis(200), child.wait()).await;

    let stderr_output = stderr_task.await.unwrap_or_default();

    if let Some(error) = call_response.get("error") {
        return Err(format!(
            "mcp call returned error: {}",
            preview_text(error.to_string().as_str(), 300)
        ));
    }

    let result = call_response.get("result").cloned().unwrap_or(Value::Null);
    Ok(json!({
        "command": command,
        "args": args,
        "method": method,
        "result": result,
        "initialize": init_response.get("result").cloned().unwrap_or(Value::Null),
        "stderr": stderr_output,
        "duration_ms": started.elapsed().as_millis() as u64,
    }))
}

async fn write_jsonrpc_line(
    stdin: &mut tokio::process::ChildStdin,
    payload: &Value,
) -> Result<(), String> {
    let mut line = serde_json::to_string(payload).map_err(|err| format!("{err}"))?;
    line.push('\n');
    stdin
        .write_all(line.as_bytes())
        .await
        .map_err(|err| format!("failed to write jsonrpc message: {err}"))?;
    stdin
        .flush()
        .await
        .map_err(|err| format!("failed to flush jsonrpc message: {err}"))?;
    Ok(())
}

async fn read_jsonrpc_response<R: AsyncRead + Unpin>(
    lines: &mut tokio::io::Lines<AsyncBufReader<R>>,
    expected_id: i64,
) -> Result<Value, String> {
    loop {
        let line = lines
            .next_line()
            .await
            .map_err(|err| format!("failed to read jsonrpc response: {err}"))?
            .ok_or_else(|| "mcp server closed stdout before sending response".to_string())?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let decoded: Value = match serde_json::from_str(trimmed) {
            Ok(value) => value,
            Err(_) => continue,
        };
        let id = decoded.get("id").and_then(Value::as_i64);
        if id == Some(expected_id) {
            return Ok(decoded);
        }
    }
}

async fn read_stream_limited<R: AsyncRead + Unpin>(stream: R, max_bytes: usize) -> String {
    let mut reader = AsyncBufReader::new(stream);
    let mut output = String::new();
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) => break,
            Ok(_) => {
                output.push_str(&line);
                if output.len() > max_bytes {
                    let truncate_to = output.len() - max_bytes;
                    output.drain(..truncate_to);
                }
            }
            Err(_) => break,
        }
    }
    output
}

fn enforce_command_policy(command: &str, allowed_commands: &HashSet<String>) -> Result<(), String> {
    if command.contains('/') || command.contains('\\') {
        return Err("command must be a bare executable name (paths are not allowed)".to_string());
    }
    let normalized = command.trim().to_lowercase();
    if !allowed_commands.contains(&normalized) {
        return Err(format!("command is not allowed by policy: {command}"));
    }
    Ok(())
}

fn resolve_workspace_root(target: &str) -> Result<PathBuf, String> {
    match ensure_workspace_target_dir(target)? {
        WorkspaceFolderEnsureOutcome::Created | WorkspaceFolderEnsureOutcome::Existing => {}
        WorkspaceFolderEnsureOutcome::Skipped => {
            return Err("session target is not a filesystem workspace path".to_string());
        }
    }
    let canonical = fs::canonicalize(target)
        .map_err(|err| format!("failed to resolve workspace target {}: {err}", target))?;
    if !canonical.is_dir() {
        return Err(format!(
            "workspace target is not a directory: {}",
            canonical.display()
        ));
    }
    Ok(canonical)
}

fn resolve_workspace_relative_path(
    workspace_root: &FsPath,
    raw_path: &str,
) -> Result<PathBuf, String> {
    let trimmed = raw_path.trim();
    if trimmed.is_empty() {
        return Err("path must not be empty".to_string());
    }
    let relative = FsPath::new(trimmed);
    if relative.is_absolute() {
        return Err("path must be relative to the workspace root".to_string());
    }

    let mut normalized = PathBuf::new();
    for component in relative.components() {
        match component {
            Component::Normal(part) => normalized.push(part),
            Component::CurDir => {}
            Component::ParentDir => {
                return Err(format!(
                    "path must not contain parent-directory traversal: {}",
                    raw_path
                ));
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err("path must be relative to the workspace root".to_string());
            }
        }
    }
    if normalized.as_os_str().is_empty() {
        return Err("path must not resolve to workspace root".to_string());
    }
    Ok(workspace_root.join(normalized))
}

async fn workspace_ensure_folder(
    Json(req): Json<WorkspaceEnsureFolderRequest>,
) -> ApiResult<WorkspaceEnsureFolderResponse> {
    let path = req.path.trim();
    if path.is_empty() {
        return Err(ApiError::bad_request("path must not be empty"));
    }
    let outcome = ensure_workspace_target_dir(path).map_err(ApiError::internal)?;
    if outcome == WorkspaceFolderEnsureOutcome::Skipped {
        return Err(ApiError::bad_request(
            "path does not look like a filesystem folder path",
        ));
    }

    Ok(Json(WorkspaceEnsureFolderResponse {
        path: path.to_string(),
        created: outcome == WorkspaceFolderEnsureOutcome::Created,
        existed: outcome == WorkspaceFolderEnsureOutcome::Existing,
    }))
}

async fn ws_events(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Query(query): Query<WsQuery>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| ws_session(socket, state, query))
}

async fn ws_session(mut socket: WebSocket, state: AppState, query: WsQuery) {
    let _token = query.token.clone();
    let mut rx = state.event_tx.subscribe();

    let hello = json!({
        "event_type": "kernel.ws.connected",
        "timestamp_ms": now_ms(),
        "filters": {
            "session_id": query.session_id,
            "task_id": query.task_id
        }
    });
    if socket
        .send(Message::Text(hello.to_string().into()))
        .await
        .is_err()
    {
        return;
    }

    loop {
        let event = match rx.recv().await {
            Ok(event) => event,
            Err(broadcast::error::RecvError::Lagged(_)) => continue,
            Err(broadcast::error::RecvError::Closed) => break,
        };

        if !ws_matches_filter(&event, &query) {
            continue;
        }

        let message = Message::Text(
            json!({
                "event_id": event.event_id,
                "event_type": event.event_type,
                "session_id": event.session_id,
                "task_id": event.task_id,
                "timestamp_ms": event.timestamp_ms,
                "payload": event.payload
            })
            .to_string()
            .into(),
        );

        if socket.send(message).await.is_err() {
            break;
        }
    }
}

fn ws_matches_filter(event: &RealtimeEvent, query: &WsQuery) -> bool {
    event_matches_filter(event, query.session_id.as_deref(), query.task_id.as_deref())
}

fn event_matches_filter(
    event: &RealtimeEvent,
    session_id: Option<&str>,
    task_id: Option<&str>,
) -> bool {
    let session_ok = session_id.map_or(true, |target| target == event.session_id);
    let task_ok = task_id.map_or(true, |target| {
        event
            .task_id
            .as_ref()
            .map_or(false, |event_task_id| event_task_id == target)
    });
    session_ok && task_ok
}

async fn file_status(
    State(state): State<AppState>,
    Path(path): Path<ProjectSessionPath>,
) -> ApiResult<FileStatusResponse> {
    let guard = state
        .inner
        .lock()
        .map_err(|_| ApiError::internal("change state mutex poisoned"))?;
    let session_key = session_key(&path.project_id, &path.session_id);
    let files = guard
        .session_changes
        .get(&session_key)
        .cloned()
        .unwrap_or_default();

    Ok(Json(FileStatusResponse {
        project_id: path.project_id,
        session_id: path.session_id,
        files: files.into_iter().map(to_status_item).collect(),
    }))
}

async fn file_view(
    State(state): State<AppState>,
    Path(path): Path<ProjectSessionPath>,
    Query(query): Query<FileViewQuery>,
) -> ApiResult<FileViewResponse> {
    let guard = state
        .inner
        .lock()
        .map_err(|_| ApiError::internal("change state mutex poisoned"))?;
    let key = session_key(&path.project_id, &path.session_id);
    let files = guard
        .session_changes
        .get(&key)
        .ok_or_else(|| ApiError::not_found(format!("session changes not found: {key}")))?;

    let exists = files.iter().any(|f| f.path == query.path);
    if !exists {
        return Err(ApiError::not_found(format!(
            "file not found in session changes: {}",
            query.path
        )));
    }

    let content = match query.view.as_str() {
        "raw" => json!({
            "content": format!("// raw view placeholder for {}\n", query.path)
        }),
        "patch" => json!({
            "content": format!(
                "diff --git a/{0} b/{0}\n--- a/{0}\n+++ b/{0}\n@@ -1 +1 @@\n-// old line\n+// new line\n",
                query.path
            )
        }),
        "diff" => json!({
            "hunks": [{
                "old_start": 1,
                "old_count": 1,
                "new_start": 1,
                "new_count": 1,
                "header": "@@ mock @@",
                "lines": [
                    {"type": "remove", "text": "// old line"},
                    {"type": "add", "text": "// new line"}
                ]
            }]
        }),
        other => {
            return Err(ApiError::bad_request(format!(
                "unsupported view '{}', expected raw|patch|diff",
                other
            )));
        }
    };

    Ok(Json(FileViewResponse {
        project_id: path.project_id,
        session_id: path.session_id,
        path: query.path,
        view: query.view,
        content,
    }))
}

async fn changes_summary(
    State(state): State<AppState>,
    Path(path): Path<ProjectSessionPath>,
) -> ApiResult<ChangeSummaryResponse> {
    let guard = state
        .inner
        .lock()
        .map_err(|_| ApiError::internal("change state mutex poisoned"))?;
    let key = session_key(&path.project_id, &path.session_id);
    let files = guard.session_changes.get(&key).cloned().unwrap_or_default();
    let additions = files.iter().map(|f| f.additions).sum();
    let deletions = files.iter().map(|f| f.deletions).sum();
    let acked = guard.acked_changes.contains(&key);
    Ok(Json(ChangeSummaryResponse {
        project_id: path.project_id,
        session_id: path.session_id,
        file_count: files.len(),
        additions,
        deletions,
        acked,
    }))
}

async fn changes_ack(
    State(state): State<AppState>,
    Path(path): Path<ProjectSessionPath>,
    Json(req): Json<ChangeAckRequest>,
) -> ApiResult<ChangeAckResponse> {
    let mut guard = state
        .inner
        .lock()
        .map_err(|_| ApiError::internal("change state mutex poisoned"))?;
    let key = session_key(&path.project_id, &path.session_id);
    guard.acked_changes.insert(key);
    guard.next_ack_id += 1;
    let ack_id = format!("ack_{:04}", guard.next_ack_id);
    drop(guard);
    state.persist();

    Ok(Json(ChangeAckResponse {
        ack_id,
        project_id: path.project_id,
        session_id: path.session_id,
        actor: req.actor.unwrap_or_else(|| "operator".to_string()),
        note: req.note,
        timestamp_ms: now_ms(),
    }))
}

fn normalize_dispatcher_input(
    kind: Option<&str>,
    reference: Option<&str>,
) -> Result<Option<(String, String)>, ApiError> {
    let kind = kind.map(str::trim).filter(|value| !value.is_empty());
    let reference = reference.map(str::trim).filter(|value| !value.is_empty());

    match (kind, reference) {
        (None, None) => Ok(None),
        (Some(_), None) | (None, Some(_)) => Err(ApiError::bad_request(
            "dispatcher_kind and dispatcher_ref must be provided together",
        )),
        (Some(kind), Some(reference)) => {
            let normalized_kind = kind.to_lowercase();
            if normalized_kind != "provider"
                && normalized_kind != "local_client"
                && normalized_kind != "ssh"
            {
                return Err(ApiError::bad_request(
                    "dispatcher_kind must be provider, local_client, or ssh",
                ));
            }
            Ok(Some((normalized_kind, reference.to_string())))
        }
    }
}

fn default_dispatcher_selection(config_db: &FsPath) -> (String, String, &'static str) {
    match runtime_config::list_providers(config_db) {
        Ok(providers) if !providers.is_empty() => (
            "provider".to_string(),
            "default".to_string(),
            "default-provider",
        ),
        _ => (
            "local_client".to_string(),
            "codex".to_string(),
            "default-local-client",
        ),
    }
}

struct KernelRoleStageExecutor {
    stream_context: Option<TaskOutputStreamContext>,
}

#[async_trait]
impl RoleStageExecutor for KernelRoleStageExecutor {
    async fn execute_stage(
        &self,
        ctx: StageExecutionContext<'_>,
    ) -> Result<StageExecutionOutput, String> {
        let mut patch = Map::new();
        patch.insert(
            format!("stage.{}.role", ctx.stage.id),
            Value::String(ctx.role.id.clone()),
        );
        patch.insert(
            format!("stage.{}.status", ctx.stage.id),
            Value::String("ok".to_string()),
        );

        match ctx.stage.kind {
            StageKind::WorkerDispatch | StageKind::Verify => {
                let session_target = ctx.state.get("session_target").and_then(Value::as_str);
                let payload = dispatch_with_target(
                    &ctx.dispatch_target.kind,
                    &ctx.dispatch_target.reference,
                    ctx.prompt.clone(),
                    session_target,
                    self.stream_context.as_ref(),
                )
                .await?;
                patch.insert("last_dispatch".to_string(), payload.clone());
                let output = payload
                    .get("output")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| payload.to_string());
                Ok(StageExecutionOutput {
                    output: Some(output),
                    state_patch: patch,
                })
            }
            StageKind::SchedulerDispatch => {
                patch.insert(
                    "scheduler.last_role".to_string(),
                    Value::String(ctx.role.id.clone()),
                );
                Ok(StageExecutionOutput {
                    output: Some("scheduler selected worker pool".to_string()),
                    state_patch: patch,
                })
            }
            StageKind::Start | StageKind::Intake | StageKind::End | StageKind::Custom => {
                Ok(StageExecutionOutput {
                    output: Some(format!(
                        "stage {} executed by {}",
                        ctx.stage.id, ctx.role.display_name
                    )),
                    state_patch: patch,
                })
            }
        }
    }
}

fn role_pipeline_path() -> PathBuf {
    std::env::var("SPIRAL_ORGAN_ROLE_PIPELINE_PATH")
        .or_else(|_| std::env::var("SPIRAL_ROLE_PIPELINE_PATH"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(".design/specs/pipeline.role.default.yaml"))
}

async fn run_role_pipeline_for_task(
    app_state: AppState,
    task_id: &str,
    session_id: &str,
    session: &SessionRecord,
    title: &str,
    input: Option<&str>,
    dispatcher_kind: &str,
    dispatcher_ref: &str,
) -> Result<agent_role_pipeline::PipelineRunResult, String> {
    let path = role_pipeline_path();
    ensure_default_pipeline_file(&path)?;
    let runner = RolePipelineRunner::from_yaml_path(&path)?;
    let executor = KernelRoleStageExecutor {
        stream_context: Some(TaskOutputStreamContext {
            state: app_state.clone(),
            task_id: task_id.to_string(),
            session_id: session_id.to_string(),
        }),
    };
    eprintln!(
        "kernel.pipeline: start task_id={} session_id={} dispatcher={}/{} pipeline_path={}",
        task_id,
        session_id,
        dispatcher_kind,
        dispatcher_ref,
        path.display()
    );

    let mut state = Map::new();
    state.insert(
        "project_id".to_string(),
        Value::String(session.project_id.clone()),
    );
    state.insert(
        "session_id".to_string(),
        Value::String(session_id.to_string()),
    );
    state.insert("task_id".to_string(), Value::String(task_id.to_string()));
    state.insert(
        "session_target".to_string(),
        Value::String(session.target.clone()),
    );
    let effective_input = compose_input_with_active_skills(input, &app_state);

    let result = runner
        .run(
            PipelineRunInput {
                task_id: task_id.to_string(),
                title: title.to_string(),
                input: effective_input,
                dispatcher_kind: dispatcher_kind.to_string(),
                dispatcher_ref: dispatcher_ref.to_string(),
                state,
            },
            &executor,
        )
        .await;
    eprintln!(
        "kernel.pipeline: finish task_id={} outcome={}",
        task_id,
        preview_pipeline_result(&result)
    );
    result
}

fn compose_input_with_active_skills(input: Option<&str>, app_state: &AppState) -> Option<String> {
    let active_skills = {
        let guard = match app_state.inner.lock() {
            Ok(guard) => guard,
            Err(_) => return input.map(str::to_string),
        };
        guard
            .skills
            .values()
            .filter(|skill| skill.active)
            .cloned()
            .collect::<Vec<_>>()
    };
    if active_skills.is_empty() {
        return input.map(str::to_string);
    }

    let mut lines = Vec::with_capacity(active_skills.len() + 2);
    lines.push("Active skills:".to_string());
    for skill in active_skills {
        let description = skill.description.trim();
        if description.is_empty() {
            lines.push(format!("- {} ({})", skill.name, skill.path));
        } else {
            lines.push(format!(
                "- {} ({}) - {}",
                skill.name, skill.path, description
            ));
        }
    }
    lines.push(String::new());
    lines.push("Skill control protocol:".to_string());
    lines.push("If you need to change skill activation, append one directive per line using this exact format:".to_string());
    lines.push("@skill activate <skill_id_or_skill_name>".to_string());
    lines.push("@skill deactivate <skill_id_or_skill_name>".to_string());
    lines.push(
        "Directives are parsed after task completion and applied by the runtime if valid."
            .to_string(),
    );

    let skill_context = lines.join("\n");
    let base = input.map(str::trim).filter(|value| !value.is_empty());
    Some(match base {
        Some(existing) => format!("{existing}\n\n{skill_context}"),
        None => skill_context,
    })
}

fn skill_store_catalog() -> Vec<SkillStoreCatalogItem> {
    vec![
        SkillStoreCatalogItem {
            item_id: "catalog.codex.default",
            name: "Codex Execution",
            description: "Use Codex CLI execution style and strict patch hygiene for repository changes.",
            path: "skills/codex-execution/SKILL.md",
        },
        SkillStoreCatalogItem {
            item_id: "catalog.release.wrapup",
            name: "Release Wrapup",
            description: "Run changelog, validation, and release checklist before final handoff.",
            path: "skills/release-wrapup/SKILL.md",
        },
        SkillStoreCatalogItem {
            item_id: "catalog.sandbox.ask_first",
            name: "Sandbox Ask-First",
            description: "Favor sandbox tool calls with explicit approval prompts and minimal risk actions.",
            path: "skills/sandbox-ask-first/SKILL.md",
        },
    ]
}

fn find_skill_for_catalog_item<'a>(
    item: &SkillStoreCatalogItem,
    skills: &'a HashMap<String, SkillRecord>,
) -> Option<&'a SkillRecord> {
    let target_path = item.path.trim();
    skills.values().find(|skill| {
        skill.path.trim() == target_path || skill.name.trim().eq_ignore_ascii_case(item.name)
    })
}

fn parse_skill_directives(text: &str) -> Vec<SkillDirective> {
    let mut directives = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if !trimmed.to_ascii_lowercase().starts_with("@skill ") {
            continue;
        }
        let mut parts = trimmed.split_whitespace();
        let command = match parts.next() {
            Some(value) => value,
            None => continue,
        };
        if !command.eq_ignore_ascii_case("@skill") {
            continue;
        }
        let action = match parts.next().map(|value| value.to_ascii_lowercase()) {
            Some(value) if value == "activate" => SkillDirectiveAction::Activate,
            Some(value) if value == "deactivate" => SkillDirectiveAction::Deactivate,
            _ => continue,
        };
        let target = parts.collect::<Vec<_>>().join(" ");
        let normalized_target = target.trim().trim_matches('"').trim_matches('\'').trim();
        if normalized_target.is_empty() {
            continue;
        }
        directives.push(SkillDirective {
            action,
            target: normalized_target.to_string(),
        });
    }
    directives
}

fn apply_ai_skill_directives(state: &AppState, session_id: &str, task_id: &str, output: &str) {
    let directives = parse_skill_directives(output);
    if directives.is_empty() {
        return;
    }

    let mut changed = false;
    for directive in directives {
        let apply_result = {
            let mut guard = match state.inner.lock() {
                Ok(guard) => guard,
                Err(_) => {
                    publish_kernel_event(
                        state,
                        session_id,
                        Some(task_id),
                        "skill.directive_failed",
                        json!({
                            "summary": "ai skill directive failed",
                            "action": directive.action.as_str(),
                            "target": directive.target,
                            "reason": "skills state mutex poisoned",
                        }),
                    );
                    return;
                }
            };

            let lookup_key = directive.target.trim();
            let lookup_key_normalized = slugify(lookup_key);
            let matched_skill_id = if guard.skills.contains_key(lookup_key) {
                Some(lookup_key.to_string())
            } else {
                guard.skills.values().find_map(|skill| {
                    if skill.name.eq_ignore_ascii_case(lookup_key)
                        || slugify(&skill.name) == lookup_key_normalized
                        || slugify(&skill.skill_id) == lookup_key_normalized
                    {
                        Some(skill.skill_id.clone())
                    } else {
                        None
                    }
                })
            };

            if let Some(skill_id) = matched_skill_id {
                if let Some(skill) = guard.skills.get_mut(&skill_id) {
                    let now = now_ms();
                    let requested_active =
                        matches!(directive.action, SkillDirectiveAction::Activate);
                    let was_active = skill.active;
                    if was_active != requested_active {
                        skill.active = requested_active;
                        skill.updated_at_ms = now;
                        changed = true;
                    }
                    Some((skill_id, skill.name.clone(), was_active, skill.active))
                } else {
                    None
                }
            } else {
                None
            }
        };

        match apply_result {
            Some((skill_id, name, was_active, is_active)) => {
                let event_type = match directive.action {
                    SkillDirectiveAction::Activate => "skill.activated_by_ai",
                    SkillDirectiveAction::Deactivate => "skill.deactivated_by_ai",
                };
                publish_kernel_event(
                    state,
                    session_id,
                    Some(task_id),
                    event_type,
                    json!({
                        "summary": "ai skill directive applied",
                        "skill_id": skill_id,
                        "skill_name": name,
                        "action": directive.action.as_str(),
                        "target": directive.target,
                        "previous_active": was_active,
                        "active": is_active,
                        "changed": was_active != is_active,
                    }),
                );
            }
            None => {
                publish_kernel_event(
                    state,
                    session_id,
                    Some(task_id),
                    "skill.directive_failed",
                    json!({
                        "summary": "ai skill directive failed",
                        "action": directive.action.as_str(),
                        "target": directive.target,
                        "reason": "skill not found",
                    }),
                );
            }
        }
    }

    if changed {
        state.persist();
    }
}

async fn dispatch_with_target(
    kind: &str,
    reference: &str,
    prompt: String,
    session_target: Option<&str>,
    stream_context: Option<&TaskOutputStreamContext>,
) -> Result<Value, String> {
    let cwd = normalize_dispatch_cwd(session_target);
    eprintln!(
        "kernel.dispatch: kind={} reference={} cwd={} prompt_preview={}",
        kind,
        reference,
        cwd.as_deref().unwrap_or("<none>"),
        preview_text(&prompt, 180)
    );
    match kind {
        "provider" => {
            let result = if let Some(context) = stream_context.cloned() {
                let (chunk_tx, mut chunk_rx) = mpsc::unbounded_channel();
                let publish_state = context.state.clone();
                let publish_task_id = context.task_id.clone();
                let publish_session_id = context.session_id.clone();
                let publisher = tokio::spawn(async move {
                    while let Some(chunk) = chunk_rx.recv().await {
                        publish_streaming_dispatch_chunk(
                            &publish_state,
                            &publish_task_id,
                            &publish_session_id,
                            chunk,
                        );
                    }
                });

                let result = dispatch_provider_prompt(
                    reference,
                    prompt,
                    Some(context.clone()),
                    Some(chunk_tx),
                )
                .await;
                let _ = publisher.await;
                result?
            } else {
                dispatch_provider_prompt(reference, prompt, None, None).await?
            };

            Ok(result)
        }
        "local_client" => {
            let request = local_client::LocalDispatchRequest {
                client_id: reference.to_string(),
                prompt,
                cwd,
                timeout_ms: Some(300_000),
            };

            let result = if let Some(context) = stream_context.cloned() {
                let (chunk_tx, mut chunk_rx) = mpsc::unbounded_channel();
                let publish_state = context.state.clone();
                let publish_task_id = context.task_id.clone();
                let publish_session_id = context.session_id.clone();
                let publisher = tokio::spawn(async move {
                    while let Some(chunk) = chunk_rx.recv().await {
                        publish_streaming_dispatch_chunk(
                            &publish_state,
                            &publish_task_id,
                            &publish_session_id,
                            chunk,
                        );
                    }
                });

                let result =
                    local_client::dispatch_prompt_with_stream(&request, Some(chunk_tx)).await;
                let _ = publisher.await;
                result?
            } else {
                local_client::dispatch_prompt(&request).await?
            };

            if !result.ok {
                return Err(result
                    .error
                    .clone()
                    .unwrap_or_else(|| "local client execution failed".to_string()));
            }
            Ok(json!({
                "client_id": result.client_id,
                "output": result.stdout,
                "stderr": result.stderr,
                "duration_ms": result.duration_ms,
                "exit_code": result.exit_code,
            }))
        }
        "ssh" => {
            let state = stream_context
                .map(|context| context.state.clone())
                .ok_or_else(|| "ssh dispatch requires a runtime state context".to_string())?;
            let target = resolve_ssh_target_for_dispatch(&state, reference)?;

            let result = if let Some(context) = stream_context.cloned() {
                let (chunk_tx, mut chunk_rx) = mpsc::unbounded_channel();
                let publish_state = context.state.clone();
                let publish_task_id = context.task_id.clone();
                let publish_session_id = context.session_id.clone();
                let publisher = tokio::spawn(async move {
                    while let Some(chunk) = chunk_rx.recv().await {
                        publish_streaming_dispatch_chunk(
                            &publish_state,
                            &publish_task_id,
                            &publish_session_id,
                            chunk,
                        );
                    }
                });
                let result = dispatch_ssh_prompt(&target, prompt, cwd, Some(chunk_tx)).await;
                let _ = publisher.await;
                result?
            } else {
                dispatch_ssh_prompt(&target, prompt, cwd, None).await?
            };

            Ok(result)
        }
        "custom" => Err("custom dispatch target is not implemented".to_string()),
        other => Err(format!("unsupported dispatcher kind: {other}")),
    }
}

const MAX_REMOTE_CAPTURE_BYTES: usize = 256 * 1024;

fn resolve_ssh_target_for_dispatch(
    state: &AppState,
    reference: &str,
) -> Result<SshTargetRecord, String> {
    let guard = state
        .inner
        .lock()
        .map_err(|_| "ssh target state mutex poisoned".to_string())?;
    if guard.ssh_targets.is_empty() {
        return Err("no ssh targets configured".to_string());
    }

    let requested = reference.trim();
    if requested.is_empty() || requested.eq_ignore_ascii_case("default") {
        let mut targets = guard.ssh_targets.values().cloned().collect::<Vec<_>>();
        targets.sort_by(|left, right| left.ssh_target_id.cmp(&right.ssh_target_id));
        return targets
            .into_iter()
            .next()
            .ok_or_else(|| "no ssh targets configured".to_string());
    }

    if let Some(target) = guard.ssh_targets.get(requested) {
        return Ok(target.clone());
    }

    let normalized = requested.to_lowercase();
    guard
        .ssh_targets
        .values()
        .find(|target| {
            target.name.to_lowercase() == normalized || target.host.to_lowercase() == normalized
        })
        .cloned()
        .ok_or_else(|| format!("ssh target not found: {reference}"))
}

async fn dispatch_ssh_prompt(
    target: &SshTargetRecord,
    prompt: String,
    cwd: Option<String>,
    stream_tx: Option<mpsc::UnboundedSender<local_client::LocalDispatchChunk>>,
) -> Result<Value, String> {
    let binary = "ssh".to_string();
    let args = build_ssh_command_args(target, &prompt, cwd.as_deref());
    let mut command = TokioCommand::new(&binary);
    command
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    eprintln!(
        "kernel.dispatch.ssh: target={} host={} args={:?}",
        target.ssh_target_id, target.host, args
    );

    let started = Instant::now();
    let mut child = command
        .spawn()
        .map_err(|err| format!("failed to execute ssh command: {err}"))?;
    let mut stdout = String::new();
    let mut stderr = String::new();
    let mut sequence = 0_u64;

    let (raw_tx, mut raw_rx) = mpsc::unbounded_channel::<DispatchRawChunk>();
    let mut stdout_reader = child.stdout.take().map(|stream| {
        let tx = raw_tx.clone();
        tokio::spawn(async move { read_dispatch_stream_chunks(stream, "stdout", tx).await })
    });
    let mut stderr_reader = child.stderr.take().map(|stream| {
        let tx = raw_tx.clone();
        tokio::spawn(async move { read_dispatch_stream_chunks(stream, "stderr", tx).await })
    });
    drop(raw_tx);

    let mut wait_fut = Box::pin(child.wait());
    let mut exit_status: Option<std::process::ExitStatus> = None;
    loop {
        tokio::select! {
            wait = &mut wait_fut, if exit_status.is_none() => {
                let status = wait.map_err(|err| format!("failed to wait ssh command: {err}"))?;
                exit_status = Some(status);
            }
            maybe_chunk = raw_rx.recv() => {
                match maybe_chunk {
                    Some(chunk) => {
                        append_limited(&mut stdout, &mut stderr, &chunk);
                        sequence += 1;
                        if let Some(tx) = stream_tx.as_ref() {
                            let _ = tx.send(local_client::LocalDispatchChunk {
                                stream: chunk.stream.to_string(),
                                text: chunk.text,
                                sequence,
                            });
                        }
                    }
                    None => {
                        if exit_status.is_some() {
                            break;
                        }
                    }
                }
            }
        }

        if exit_status.is_some() && raw_rx.is_closed() && raw_rx.is_empty() {
            break;
        }
    }

    if let Some(handle) = stdout_reader.take() {
        let _ = handle.await;
    }
    if let Some(handle) = stderr_reader.take() {
        let _ = handle.await;
    }

    let exit_code = exit_status.and_then(|status| status.code());
    let duration_ms = started.elapsed().as_millis() as u64;
    if exit_code != Some(0) {
        return Err(format!(
            "ssh dispatch failed target={} host={} exit_code={:?} stderr={}",
            target.ssh_target_id,
            target.host,
            exit_code,
            preview_text(&stderr, 240)
        ));
    }

    Ok(json!({
        "ssh_target_id": target.ssh_target_id,
        "name": target.name,
        "host": target.host,
        "output": stdout,
        "stderr": stderr,
        "duration_ms": duration_ms,
        "exit_code": exit_code,
    }))
}

fn build_ssh_command_args(
    target: &SshTargetRecord,
    prompt: &str,
    cwd: Option<&str>,
) -> Vec<String> {
    let mut args = Vec::new();
    if let Some(port) = target.port {
        args.push("-p".to_string());
        args.push(port.to_string());
    }
    if let Some(identity_file) = target.identity_file.as_deref() {
        args.push("-i".to_string());
        args.push(identity_file.to_string());
    }
    for option in &target.options {
        args.push(option.clone());
    }
    args.push(build_ssh_destination(target));
    args.push(build_ssh_remote_command(target, prompt, cwd));
    args
}

fn build_ssh_destination(target: &SshTargetRecord) -> String {
    let host = target.host.trim();
    if let Some(username) = target
        .username
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        format!("{username}@{host}")
    } else {
        host.to_string()
    }
}

fn build_ssh_remote_command(target: &SshTargetRecord, prompt: &str, cwd: Option<&str>) -> String {
    let prompt_value = shell_quote_single(prompt);
    let remote_cwd = target
        .remote_workdir
        .as_deref()
        .or(cwd)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(shell_quote_single);

    let mut script = String::new();
    if let Some(cwd) = remote_cwd {
        script.push_str("cd ");
        script.push_str(&cwd);
        script.push_str(" && ");
    }
    script.push_str(
        "if command -v codex >/dev/null 2>&1; then \
codex --dangerously-bypass-approvals-and-sandbox exec --json --skip-git-repo-check ",
    );
    script.push_str(&prompt_value);
    script.push_str(
        "; else \
printf '%s\\n' ",
    );
    script.push_str(&prompt_value);
    script.push_str("; fi");
    script
}

fn shell_quote_single(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

async fn read_dispatch_stream_chunks<R: AsyncRead + Unpin>(
    stream: R,
    channel: &'static str,
    tx: mpsc::UnboundedSender<DispatchRawChunk>,
) {
    let mut reader = AsyncBufReader::new(stream);
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) => break,
            Ok(_) => {
                let _ = tx.send(DispatchRawChunk {
                    stream: channel,
                    text: line.clone(),
                });
            }
            Err(_) => break,
        }
    }
}

fn append_limited(stdout: &mut String, stderr: &mut String, chunk: &DispatchRawChunk) {
    let target = if chunk.stream == "stderr" {
        stderr
    } else {
        stdout
    };
    target.push_str(&chunk.text);
    if target.len() > MAX_REMOTE_CAPTURE_BYTES {
        let truncate_to = target.len() - MAX_REMOTE_CAPTURE_BYTES;
        target.drain(..truncate_to);
    }
}

async fn dispatch_provider_prompt(
    provider_ref: &str,
    prompt: String,
    context: Option<TaskOutputStreamContext>,
    stream_tx: Option<mpsc::UnboundedSender<local_client::LocalDispatchChunk>>,
) -> Result<Value, String> {
    let db_path = runtime_config::default_db_path();
    let providers = runtime_config::list_providers(&db_path)?;
    if providers.is_empty() {
        return Err("no providers configured".to_string());
    }

    let selected = resolve_provider_for_dispatch(provider_ref, &providers)?;
    let provider_name = selected.name.clone();
    let provider_id = selected.id;

    let output = if matches!(selected.kind.as_str(), "openai" | "openrouter" | "custom") {
        provider_agent::dispatch_openai_like_provider_agent(
            context,
            selected.clone(),
            prompt,
            stream_tx.clone(),
        )
        .await?
    } else if selected.kind == "anthropic" {
        provider_agent::dispatch_anthropic_provider_agent(
            context,
            selected.clone(),
            prompt,
            stream_tx.clone(),
        )
        .await?
    } else {
        let selected_clone = selected.clone();
        let output = tokio::task::spawn_blocking(move || {
            let runtime = RuntimeProvider::from_config(&selected_clone)?;
            let response = runtime
                .complete(ModelRequest { prompt })
                .map_err(|e| e.to_string())?;
            Ok::<String, String>(response.output)
        })
        .await
        .map_err(|e| format!("provider dispatch join error: {e}"))??;
        if let Some(tx) = stream_tx.as_ref() {
            let _ = tx.send(local_client::LocalDispatchChunk {
                stream: "stdout".to_string(),
                text: output.clone(),
                sequence: 1,
            });
        }
        output
    };

    Ok(json!({
        "provider_id": provider_id,
        "provider_name": provider_name,
        "output": output,
    }))
}

#[allow(dead_code)]
fn dispatch_openai_like_provider_streaming(
    config: runtime_config::ProviderConfig,
    prompt: String,
    stream_tx: Option<mpsc::UnboundedSender<local_client::LocalDispatchChunk>>,
) -> Result<String, String> {
    let api_key = config
        .api_key
        .clone()
        .ok_or_else(|| format!("provider api_key is required for {} kind", config.kind))?;
    let default_base_url = match config.kind.as_str() {
        "openrouter" => "https://openrouter.ai/api/v1",
        _ => "https://api.openai.com/v1",
    };
    let base_url = config
        .base_url
        .clone()
        .unwrap_or_else(|| default_base_url.to_string());
    let endpoint = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let client = reqwest::blocking::Client::new();

    let response = client
        .post(endpoint)
        .bearer_auth(api_key)
        .json(&json!({
            "model": config.model,
            "stream": true,
            "messages": [
                {
                    "role": "user",
                    "content": prompt.clone()
                }
            ]
        }))
        .send()
        .map_err(|e| format!("provider request failed: {e}"))?;

    let status = response.status();
    if !status.is_success() {
        let body = response
            .text()
            .unwrap_or_else(|_| "<failed to read error body>".to_string());
        return Err(format!("provider returned {}: {}", status, body));
    }

    let mut reader = StdBufReader::new(response);
    let mut line = String::new();
    let mut sequence = 0_u64;
    let mut output = String::new();

    loop {
        line.clear();
        let read = reader
            .read_line(&mut line)
            .map_err(|e| format!("provider stream read failed: {e}"))?;
        if read == 0 {
            break;
        }

        let trimmed = line.trim();
        if trimmed.is_empty() || !trimmed.starts_with("data:") {
            continue;
        }

        let payload = trimmed.trim_start_matches("data:").trim();
        if payload == "[DONE]" {
            break;
        }

        let value: Value = match serde_json::from_str(payload) {
            Ok(value) => value,
            Err(_) => continue,
        };
        let chunk_text = value
            .get("choices")
            .and_then(|choices| choices.get(0))
            .and_then(|choice| choice.get("delta"))
            .and_then(|delta| delta.get("content"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if chunk_text.is_empty() {
            continue;
        }

        output.push_str(&chunk_text);
        sequence += 1;
        if let Some(tx) = stream_tx.as_ref() {
            let _ = tx.send(local_client::LocalDispatchChunk {
                stream: "stdout".to_string(),
                text: chunk_text,
                sequence,
            });
        }
    }

    if output.trim().is_empty() {
        output = dispatch_openai_like_provider_single(&config, &prompt)?;
        if let Some(tx) = stream_tx.as_ref() {
            let _ = tx.send(local_client::LocalDispatchChunk {
                stream: "stdout".to_string(),
                text: output.clone(),
                sequence: sequence.saturating_add(1),
            });
        }
    }

    Ok(output)
}

#[allow(dead_code)]
fn dispatch_openai_like_provider_single(
    config: &runtime_config::ProviderConfig,
    prompt: &str,
) -> Result<String, String> {
    let api_key = config
        .api_key
        .clone()
        .ok_or_else(|| format!("provider api_key is required for {} kind", config.kind))?;
    let default_base_url = match config.kind.as_str() {
        "openrouter" => "https://openrouter.ai/api/v1",
        _ => "https://api.openai.com/v1",
    };
    let base_url = config
        .base_url
        .clone()
        .unwrap_or_else(|| default_base_url.to_string());
    let endpoint = format!("{}/chat/completions", base_url.trim_end_matches('/'));

    let response = reqwest::blocking::Client::new()
        .post(endpoint)
        .bearer_auth(api_key)
        .json(&json!({
            "model": config.model,
            "messages": [
                {
                    "role": "user",
                    "content": prompt
                }
            ]
        }))
        .send()
        .map_err(|e| format!("provider request failed: {e}"))?;

    let status = response.status();
    if !status.is_success() {
        let body = response
            .text()
            .unwrap_or_else(|_| "<failed to read error body>".to_string());
        return Err(format!("provider returned {}: {}", status, body));
    }

    let payload: Value = response
        .json()
        .map_err(|e| format!("invalid provider response json: {e}"))?;
    let output = payload
        .get("choices")
        .and_then(|choices| choices.get(0))
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if output.is_empty() {
        return Err("provider response has empty message content".to_string());
    }
    Ok(output)
}

fn publish_streaming_dispatch_chunk(
    state: &AppState,
    task_id: &str,
    session_id: &str,
    chunk: local_client::LocalDispatchChunk,
) {
    let text = match extract_streaming_chunk_text(&chunk) {
        Some(text) => text,
        None => return,
    };

    let event_type = if chunk.stream.eq_ignore_ascii_case("stderr") {
        "task.error.chunk"
    } else {
        "task.output.chunk"
    };

    let event = {
        let mut guard = match state.inner.lock() {
            Ok(guard) => guard,
            Err(_) => return,
        };
        append_event(
            &mut guard,
            task_id,
            event_type,
            session_id,
            json!({
                "summary": "streaming local client output",
                "stream": chunk.stream,
                "sequence": chunk.sequence,
                "text": text,
            }),
        )
    };
    state.publish_event(RealtimeEvent::from_task_event(&event));
}

fn extract_streaming_chunk_text(chunk: &local_client::LocalDispatchChunk) -> Option<String> {
    let raw = chunk.text.trim();
    if raw.is_empty() {
        return None;
    }

    if chunk.stream.eq_ignore_ascii_case("stderr") || !raw.starts_with('{') {
        return Some(raw.to_string());
    }

    let decoded: Value = match serde_json::from_str(raw) {
        Ok(value) => value,
        Err(_) => return Some(raw.to_string()),
    };

    if let Some(item) = decoded.get("item").and_then(Value::as_object) {
        let item_type = item.get("type").and_then(Value::as_str).unwrap_or_default();
        let candidate = match item_type {
            "agent_message" | "reasoning" => item.get("text").and_then(Value::as_str),
            "command_execution" => item
                .get("aggregated_output")
                .and_then(Value::as_str)
                .or_else(|| item.get("text").and_then(Value::as_str)),
            _ => None,
        };
        if let Some(text) = candidate.map(str::trim).filter(|text| !text.is_empty()) {
            return Some(text.to_string());
        }
        return None;
    }

    for key in ["text", "message", "error", "reason"] {
        if let Some(value) = decoded.get(key).and_then(Value::as_str) {
            let text = value.trim();
            if !text.is_empty() {
                return Some(text.to_string());
            }
        }
    }

    None
}

fn resolve_provider_for_dispatch(
    provider_ref: &str,
    providers: &[runtime_config::ProviderConfig],
) -> Result<runtime_config::ProviderConfig, String> {
    let reference = provider_ref.trim();
    if reference.is_empty() || reference == "default" {
        return Ok(providers[0].clone());
    }
    let parsed_id = reference
        .parse::<i64>()
        .map_err(|_| format!("provider reference must be numeric id or 'default': {reference}"))?;
    providers
        .iter()
        .find(|provider| provider.id == parsed_id)
        .cloned()
        .ok_or_else(|| format!("provider not found for id {parsed_id}"))
}

fn normalize_dispatch_cwd(session_target: Option<&str>) -> Option<String> {
    let target = session_target?.trim();
    if target.is_empty() {
        return None;
    }
    match ensure_workspace_target_dir(target) {
        Ok(WorkspaceFolderEnsureOutcome::Created) | Ok(WorkspaceFolderEnsureOutcome::Existing) => {
            Some(target.to_string())
        }
        Ok(WorkspaceFolderEnsureOutcome::Skipped) => None,
        Err(err) => {
            eprintln!(
                "kernel.dispatch: failed to ensure cwd target={} error={}",
                target, err
            );
            None
        }
    }
}

fn ensure_workspace_target_dir(target: &str) -> Result<WorkspaceFolderEnsureOutcome, String> {
    let trimmed = target.trim();
    if trimmed.is_empty() {
        return Ok(WorkspaceFolderEnsureOutcome::Skipped);
    }

    let looks_like_path = trimmed.contains(std::path::MAIN_SEPARATOR)
        || trimmed.contains('/')
        || trimmed.contains('\\')
        || trimmed.starts_with('.')
        || trimmed.starts_with('~')
        || !trimmed.contains(' ');
    if !looks_like_path {
        return Ok(WorkspaceFolderEnsureOutcome::Skipped);
    }

    let path = FsPath::new(trimmed);
    if path.exists() {
        if !path.is_dir() {
            return Err(format!(
                "target path exists but is not a directory: {}",
                path.display()
            ));
        }
        return Ok(WorkspaceFolderEnsureOutcome::Existing);
    }

    std::fs::create_dir_all(path)
        .map_err(|err| format!("failed to create target folder {}: {}", path.display(), err))?;
    Ok(WorkspaceFolderEnsureOutcome::Created)
}

fn preview_text(text: &str, max_chars: usize) -> String {
    let mut preview = text
        .chars()
        .take(max_chars.saturating_sub(16))
        .collect::<String>()
        .replace('\n', "\\n");
    if text.chars().count() > max_chars {
        preview.push_str("...(truncated)");
    }
    preview
}

fn preview_value(value: &Value, max_chars: usize) -> String {
    preview_text(&value.to_string(), max_chars)
}

fn preview_pipeline_result(
    result: &Result<agent_role_pipeline::PipelineRunResult, String>,
) -> String {
    match result {
        Ok(value) => format!(
            "ok status={} stages={} final_output_present={} error_present={}",
            value.status,
            value.stages.len(),
            value
                .final_output
                .as_ref()
                .map(|output| !output.trim().is_empty())
                .unwrap_or(false),
            value.error.is_some()
        ),
        Err(err) => format!("error {}", preview_text(err, 160)),
    }
}

fn default_persistence_path() -> PathBuf {
    std::env::var("SPIRAL_ORGAN_CORE_STATE_PATH")
        .or_else(|_| std::env::var("SPIRAL_CORE_STATE_PATH"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(".runtime/spiral_organ_core/server-state.json"))
}

fn load_persisted_state(path: &FsPath) -> Result<InMemoryApiState, String> {
    if !path.exists() {
        return Ok(InMemoryApiState::default());
    }
    let content =
        fs::read_to_string(path).map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    let persisted: PersistedState =
        serde_json::from_str(&content).map_err(|e| format!("invalid state json: {e}"))?;
    Ok(InMemoryApiState {
        next_session_id: persisted.next_session_id,
        next_task_id: persisted.next_task_id,
        next_event_id: persisted.next_event_id,
        next_ack_id: persisted.next_ack_id,
        next_deploy_id: persisted.next_deploy_id,
        next_skill_id: persisted.next_skill_id,
        next_approval_id: persisted.next_approval_id,
        next_ssh_target_id: persisted.next_ssh_target_id,
        sessions: persisted.sessions,
        tasks: persisted.tasks,
        task_events: persisted.task_events,
        session_changes: persisted
            .session_changes
            .into_iter()
            .map(|(k, v)| {
                (
                    k,
                    v.into_iter()
                        .map(|f| ChangeFile {
                            path: f.path,
                            status: parse_change_status(&f.status),
                            additions: f.additions,
                            deletions: f.deletions,
                            task_id: f.task_id,
                            agent_id: f.agent_id,
                        })
                        .collect(),
                )
            })
            .collect(),
        acked_changes: persisted.acked_changes.into_iter().collect(),
        skills: persisted.skills,
        pending_approvals: persisted.pending_approvals,
        ssh_targets: persisted.ssh_targets,
    })
}

fn persist_state(path: &FsPath, state: &InMemoryApiState) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create state dir {}: {e}", parent.display()))?;
    }

    let persisted = PersistedState {
        next_session_id: state.next_session_id,
        next_task_id: state.next_task_id,
        next_event_id: state.next_event_id,
        next_ack_id: state.next_ack_id,
        next_deploy_id: state.next_deploy_id,
        next_skill_id: state.next_skill_id,
        next_approval_id: state.next_approval_id,
        next_ssh_target_id: state.next_ssh_target_id,
        sessions: state.sessions.clone(),
        tasks: state.tasks.clone(),
        task_events: state.task_events.clone(),
        session_changes: state
            .session_changes
            .iter()
            .map(|(k, v)| {
                (
                    k.clone(),
                    v.iter()
                        .map(|f| PersistedChangeFile {
                            path: f.path.clone(),
                            status: format_status(f.status).to_string(),
                            additions: f.additions,
                            deletions: f.deletions,
                            task_id: f.task_id.clone(),
                            agent_id: f.agent_id.clone(),
                        })
                        .collect(),
                )
            })
            .collect(),
        acked_changes: state.acked_changes.iter().cloned().collect(),
        skills: state.skills.clone(),
        pending_approvals: state.pending_approvals.clone(),
        ssh_targets: state.ssh_targets.clone(),
    };

    let body = serde_json::to_string_pretty(&persisted)
        .map_err(|e| format!("encode state failed: {e}"))?;
    fs::write(path, body).map_err(|e| format!("failed to write {}: {e}", path.display()))
}

fn append_event(
    state: &mut InMemoryApiState,
    task_id: &str,
    event_type: &str,
    session_id: &str,
    payload: Value,
) -> TaskEvent {
    state.next_event_id += 1;
    let event = TaskEvent {
        event_id: format!("evt_{:05}", state.next_event_id),
        event_type: event_type.to_string(),
        session_id: session_id.to_string(),
        task_id: task_id.to_string(),
        timestamp_ms: now_ms(),
        payload,
    };
    state
        .task_events
        .entry(task_id.to_string())
        .or_default()
        .push(event.clone());
    event
}

fn bootstrap_changes(session_id: &str) -> Vec<ChangeFile> {
    vec![ChangeFile {
        path: "README.md".to_string(),
        status: ChangeStatus::Modified,
        additions: 2,
        deletions: 0,
        task_id: format!("{session_id}-bootstrap"),
        agent_id: "bootstrap-worker".to_string(),
    }]
}

fn session_key(project_id: &str, session_id: &str) -> String {
    format!("{project_id}:{session_id}")
}

fn to_status_item(file: ChangeFile) -> FileStatusItem {
    FileStatusItem {
        path: file.path,
        status: format_status(file.status).to_string(),
        additions: file.additions,
        deletions: file.deletions,
        task_id: file.task_id,
        agent_id: file.agent_id,
    }
}

fn format_status(status: ChangeStatus) -> &'static str {
    match status {
        ChangeStatus::Added => "added",
        ChangeStatus::Modified => "modified",
        ChangeStatus::Deleted => "deleted",
        ChangeStatus::Renamed => "renamed",
    }
}

fn parse_change_status(status: &str) -> ChangeStatus {
    match status {
        "added" => ChangeStatus::Added,
        "deleted" => ChangeStatus::Deleted,
        "renamed" => ChangeStatus::Renamed,
        _ => ChangeStatus::Modified,
    }
}

fn slugify(input: &str) -> String {
    input
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect()
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::{
        AppState, InMemoryApiState, RealtimeEvent, SessionRecord, TaskRecord, WsQuery,
        invoke_http_like, load_persisted_state, persist_state, ws_matches_filter,
    };
    use serde_json::json;

    static NEXT_SANDBOX_WORKSPACE_ID: AtomicU64 = AtomicU64::new(1);

    fn unique_sandbox_workspace_root() -> PathBuf {
        let seq = NEXT_SANDBOX_WORKSPACE_ID.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "spiral-sandbox-{}-{}-{}",
            std::process::id(),
            super::now_ms(),
            seq
        ))
    }

    #[test]
    fn ws_filter_matches_without_constraints() {
        let event = RealtimeEvent {
            event_id: "evt_1".to_string(),
            event_type: "task.done".to_string(),
            session_id: "sess_1".to_string(),
            task_id: Some("task_1".to_string()),
            timestamp_ms: 1,
            payload: json!({"summary":"ok"}),
        };
        let query = WsQuery::default();
        assert!(ws_matches_filter(&event, &query));
    }

    #[test]
    fn ws_filter_matches_session_and_task() {
        let event = RealtimeEvent {
            event_id: "evt_2".to_string(),
            event_type: "task.done".to_string(),
            session_id: "sess_1".to_string(),
            task_id: Some("task_1".to_string()),
            timestamp_ms: 2,
            payload: json!({"summary":"ok"}),
        };
        let query = WsQuery {
            token: None,
            session_id: Some("sess_1".to_string()),
            task_id: Some("task_1".to_string()),
        };
        assert!(ws_matches_filter(&event, &query));
    }

    #[test]
    fn ws_filter_rejects_mismatch() {
        let event = RealtimeEvent {
            event_id: "evt_3".to_string(),
            event_type: "deploy.finished".to_string(),
            session_id: "sess_a".to_string(),
            task_id: None,
            timestamp_ms: 3,
            payload: json!({"status":"accepted"}),
        };
        let query = WsQuery {
            token: None,
            session_id: Some("sess_b".to_string()),
            task_id: None,
        };
        assert!(!ws_matches_filter(&event, &query));
    }

    #[test]
    fn persisted_state_roundtrip() {
        let unique = format!("spiral_organ_core-test-{}.json", super::now_ms());
        let path = std::env::temp_dir().join(unique);

        let mut state = InMemoryApiState {
            next_session_id: 1,
            next_task_id: 2,
            next_event_id: 3,
            next_ack_id: 4,
            next_deploy_id: 5,
            next_skill_id: 6,
            next_approval_id: 7,
            next_ssh_target_id: 8,
            sessions: HashMap::new(),
            tasks: HashMap::new(),
            task_events: HashMap::new(),
            session_changes: HashMap::new(),
            acked_changes: HashSet::new(),
            skills: HashMap::new(),
            pending_approvals: HashMap::new(),
            ssh_targets: HashMap::new(),
        };
        state.sessions.insert(
            "sess_0001".to_string(),
            SessionRecord {
                project_id: "demo".to_string(),
                target: "persist target".to_string(),
                dispatcher_kind: Some("provider".to_string()),
                dispatcher_ref: Some("default".to_string()),
            },
        );
        state.tasks.insert(
            "task_0001".to_string(),
            TaskRecord {
                task_id: "task_0001".to_string(),
                session_id: "sess_0001".to_string(),
                title: "persist task".to_string(),
                status: "done".to_string(),
            },
        );

        persist_state(&path, &state).expect("state should persist");
        let restored = load_persisted_state(&path).expect("state should load");
        assert_eq!(restored.next_deploy_id, 5);
        assert_eq!(restored.next_skill_id, 6);
        assert_eq!(restored.next_approval_id, 7);
        assert_eq!(restored.next_ssh_target_id, 8);
        assert!(restored.sessions.contains_key("sess_0001"));
        assert!(restored.tasks.contains_key("task_0001"));

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn parse_skill_directives_extracts_activate_and_deactivate() {
        let output = r#"
analysis complete
@skill activate skill_0001
@skill deactivate "Release Wrapup"
"#;

        let directives = super::parse_skill_directives(output);
        assert_eq!(directives.len(), 2);
        assert_eq!(directives[0].target, "skill_0001");
        assert_eq!(directives[1].target, "Release Wrapup");
    }

    #[test]
    fn install_skill_store_item_creates_skill_record() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime should build");
        let state = AppState::default();

        let install = runtime
            .block_on(invoke_http_like(
                Some(state.clone()),
                "POST",
                "/kernel/skills/store/install",
                Some(json!({
                    "item_id": "catalog.codex.default"
                })),
            ))
            .expect("install should succeed");
        assert_eq!(
            install.get("status").and_then(|value| value.as_str()),
            Some("installed")
        );

        let skills = runtime
            .block_on(invoke_http_like(
                Some(state.clone()),
                "GET",
                "/kernel/skills",
                None,
            ))
            .expect("skills list should succeed");
        let list = skills
            .get("skills")
            .and_then(|value| value.as_array())
            .expect("skills should be an array");
        assert!(!list.is_empty());
    }

    #[test]
    fn runtime_ssh_target_crud_roundtrip() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime should build");
        let state = AppState::default();

        let created = runtime
            .block_on(invoke_http_like(
                Some(state.clone()),
                "POST",
                "/kernel/runtime/ssh-targets",
                Some(json!({
                    "name": "qa-box",
                    "host": "10.0.0.5",
                    "username": "dev"
                })),
            ))
            .expect("ssh target create should succeed");
        let ssh_target_id = created
            .get("ssh_target_id")
            .and_then(|value| value.as_str())
            .expect("ssh_target_id should exist")
            .to_string();

        let listed = runtime
            .block_on(invoke_http_like(
                Some(state.clone()),
                "GET",
                "/kernel/runtime/ssh-targets",
                None,
            ))
            .expect("ssh target list should succeed");
        let targets = listed
            .get("targets")
            .and_then(|value| value.as_array())
            .expect("targets should be an array");
        assert!(targets.iter().any(|entry| {
            entry
                .get("ssh_target_id")
                .and_then(|value| value.as_str())
                .map(|value| value == ssh_target_id)
                .unwrap_or(false)
        }));

        let update_path = format!("/kernel/runtime/ssh-targets/{ssh_target_id}");
        let updated = runtime
            .block_on(invoke_http_like(
                Some(state.clone()),
                "POST",
                &update_path,
                Some(json!({
                    "name": "qa-box",
                    "host": "10.0.0.5",
                    "port": 2222,
                    "username": "devops"
                })),
            ))
            .expect("ssh target update should succeed");
        assert_eq!(
            updated.get("status").and_then(|value| value.as_str()),
            Some("updated")
        );

        let deleted = runtime
            .block_on(invoke_http_like(
                Some(state.clone()),
                "DELETE",
                &update_path,
                None,
            ))
            .expect("ssh target delete should succeed");
        assert_eq!(
            deleted.get("status").and_then(|value| value.as_str()),
            Some("deleted")
        );
    }

    #[test]
    fn sandbox_execute_and_approve_writes_file() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime should build");
        let state = AppState::default();
        let workspace = unique_sandbox_workspace_root();
        fs::create_dir_all(&workspace).expect("workspace should be created");

        let session_response = runtime
            .block_on(invoke_http_like(
                Some(state.clone()),
                "POST",
                "/kernel/session/open",
                Some(json!({
                    "project_id": "sandbox-project",
                    "target": workspace.display().to_string()
                })),
            ))
            .expect("session open should succeed");
        let session_id = session_response
            .get("session_id")
            .and_then(|value| value.as_str())
            .expect("session id should exist")
            .to_string();

        let execute = runtime
            .block_on(invoke_http_like(
                Some(state.clone()),
                "POST",
                "/kernel/sandbox/tools/execute",
                Some(json!({
                    "session_id": session_id,
                    "operation": "file_write",
                    "path": "notes/hello.txt",
                    "content": "hello sandbox"
                })),
            ))
            .expect("sandbox execute should succeed");
        let execute_status = execute.get("status").and_then(|value| value.as_str());

        if execute_status == Some("approval_required") {
            let approval_id = execute
                .get("approval_id")
                .and_then(|value| value.as_str())
                .expect("approval id should exist")
                .to_string();
            let approve_path = format!("/kernel/sandbox/approvals/{approval_id}/approve");
            let approved = runtime
                .block_on(invoke_http_like(
                    Some(state.clone()),
                    "POST",
                    &approve_path,
                    Some(json!({
                        "actor": "test"
                    })),
                ))
                .expect("approval should execute operation");
            assert_eq!(
                approved.get("status").and_then(|value| value.as_str()),
                Some("executed")
            );
        } else {
            assert_eq!(execute_status, Some("executed"));
        }

        let canonical_workspace = fs::canonicalize(&workspace).expect("workspace should resolve");
        let file_path = canonical_workspace.join("notes/hello.txt");
        let content = fs::read_to_string(&file_path).expect("file should be written");
        assert_eq!(content, "hello sandbox");

        let _ = fs::remove_dir_all(workspace);
    }

    #[test]
    fn sandbox_rejects_parent_traversal_path() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime should build");
        let state = AppState::default();
        let workspace = unique_sandbox_workspace_root();
        fs::create_dir_all(&workspace).expect("workspace should be created");

        let session_response = runtime
            .block_on(invoke_http_like(
                Some(state.clone()),
                "POST",
                "/kernel/session/open",
                Some(json!({
                    "project_id": "sandbox-project",
                    "target": workspace.display().to_string()
                })),
            ))
            .expect("session open should succeed");
        let session_id = session_response
            .get("session_id")
            .and_then(|value| value.as_str())
            .expect("session id should exist")
            .to_string();

        let execute = runtime.block_on(invoke_http_like(
            Some(state.clone()),
            "POST",
            "/kernel/sandbox/tools/execute",
            Some(json!({
                "session_id": session_id,
                "operation": "file_write",
                "path": "../escape.txt",
                "content": "should fail"
            })),
        ));

        match execute {
            Ok(value) => {
                let status = value.get("status").and_then(|entry| entry.as_str());
                assert_eq!(status, Some("approval_required"));
                let approval_id = value
                    .get("approval_id")
                    .and_then(|entry| entry.as_str())
                    .expect("approval id should exist")
                    .to_string();
                let approve_path = format!("/kernel/sandbox/approvals/{approval_id}/approve");
                let result = runtime.block_on(invoke_http_like(
                    Some(state.clone()),
                    "POST",
                    &approve_path,
                    Some(json!({})),
                ));
                assert!(result.is_err());
                let message = result.err().unwrap_or_default();
                assert!(
                    message.contains("parent-directory traversal"),
                    "unexpected error: {message}"
                );
            }
            Err(message) => {
                assert!(
                    message.contains("parent-directory traversal"),
                    "unexpected error: {message}"
                );
            }
        }

        let _ = fs::remove_dir_all(workspace);
    }

    #[test]
    fn invoke_http_like_handles_open_session() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime should build");
        let response = runtime
            .block_on(invoke_http_like(
                Some(AppState::default()),
                "POST",
                "/kernel/session/open",
                Some(json!({
                    "project_id": "demo-project",
                    "target": "demo-target"
                })),
            ))
            .expect("invoke should succeed");

        assert_eq!(
            response.get("project_id").and_then(|v| v.as_str()),
            Some("demo-project")
        );
        assert!(
            response
                .get("session_id")
                .and_then(|v| v.as_str())
                .is_some()
        );
    }
}
