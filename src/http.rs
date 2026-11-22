//! HTTP server: REST API + embedded kanban web UI.
//! Web UI isn't scoped to a single agent; it browses all workspaces (agent="all" views
//! use a wildcard). API requests carry the workspace via ?ws= or X-Agent-Kb-Workspace header.

use crate::model::*;
use crate::store::Store;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    routing::{get, patch, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use tower_http::cors::CorsLayer;

#[derive(Clone)]
struct AppState {
    store: Arc<Store>,
}

fn err(status: StatusCode, msg: &str) -> Response {
    (status, Json(json!({ "ok": false, "error": msg }))).into_response()
}

#[derive(Deserialize)]
struct WsQuery {
    ws: Option<String>,
    q: Option<String>,
    status: Option<String>,
    tag: Option<String>,
}

fn pick_ws(q: &WsQuery, headers: &axum::http::HeaderMap) -> String {
    let raw = if let Some(w) = &q.ws { if !w.is_empty() { w.clone() }
        else { return "default".to_string(); } }
    else if let Some(h) = headers.get("x-agent-kb-workspace").and_then(|v| v.to_str().ok()) {
        if !h.is_empty() { h.to_string() } else { "default".to_string() }
    } else { "default".to_string() };
    crate::store::slug(&raw)
}

pub async fn serve(store: Arc<Store>, host: &str, port: u16) {
    let state = AppState { store };
    let app = Router::new()
        .route("/", get(index))
        .route("/favicon.ico", get(favicon))
        .route("/api/health", get(health))
        .route("/api/workspaces", get(list_workspaces))
        .route("/api/overview", get(overview))
        .route("/api/projects", get(list_projects).post(create_project))
        .route("/api/projects/:id", get(get_project).patch(patch_project).delete(delete_project))
        .route("/api/projects/:id/hosts", post(add_host))
        .route("/api/projects/:id/materials", post(add_material))
        .route("/api/projects/:id/decisions", post(add_decision))
        .route("/api/projects/:id/events", post(add_timeline))
        .route("/api/projects/:id/tasks", post(add_task))
        .route("/api/projects/:id/tasks/reorder", post(reorder_tasks))
        .route("/api/projects/:id/tasks/:tid", patch(patch_task).delete(del_task))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = format!("{}:{}", host, port);
    let listener = tokio::net::TcpListener::bind(&addr).await.expect("bind failed");
    eprintln!("[agent-kanban] http listening on http://{}", addr);
    axum::serve(listener, app).await.unwrap();
}

async fn index() -> Html<&'static str> {
    Html(crate::webui::INDEX_HTML)
}

async fn favicon() -> impl IntoResponse {
    // tiny svg favicon
    const SVG: &str = concat!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 32 32\">",
        "<rect width=\"32\" height=\"32\" rx=\"7\" fill=\"#5b8def\"/>",
        "<rect x=\"7\" y=\"8\" width=\"5\" height=\"16\" rx=\"1.5\" fill=\"#fff\"/>",
        "<rect x=\"14\" y=\"8\" width=\"5\" height=\"11\" rx=\"1.5\" fill=\"#fff\" opacity=\".85\"/>",
        "<rect x=\"21\" y=\"8\" width=\"5\" height=\"7\" rx=\"1.5\" fill=\"#fff\" opacity=\".7\"/>",
        "</svg>"
    );
    ([(axum::http::header::CONTENT_TYPE, "image/svg+xml")], SVG).into_response()
}

async fn health(State(s): State<AppState>) -> Json<Value> {
    let _ = &s;
    Json(json!({ "ok": true, "version": env!("CARGO_PKG_VERSION") }))
}

async fn list_workspaces(State(s): State<AppState>) -> Json<Value> {
    Json(json!({ "ok": true, "items": s.store.list_workspaces() }))
}

async fn overview(State(s): State<AppState>, Query(q): Query<WsQuery>, headers: axum::http::HeaderMap) -> Json<Value> {
    let ws = pick_ws(&q, &headers);
    Json(json!({ "ok": true, "overview": s.store.workspace_overview(&ws) }))
}

async fn list_projects(State(s): State<AppState>, Query(q): Query<WsQuery>, headers: axum::http::HeaderMap) -> Json<Value> {
    let ws = pick_ws(&q, &headers);
    let items = s.store.list_projects(&ws, q.q.as_deref(), q.status.as_deref(), q.tag.as_deref());
    Json(json!({ "ok": true, "workspace": ws, "items": items }))
}

#[derive(Deserialize)]
struct NewProject {
    name: String,
    #[serde(default)] goal: String,
    #[serde(default)] status: Option<String>,
    #[serde(default)] tags: Option<Vec<String>>,
    #[serde(default)] ws: Option<String>,
}

async fn create_project(State(s): State<AppState>, Query(q): Query<WsQuery>, headers: axum::http::HeaderMap, Json(b): Json<NewProject>) -> Response {
    let ws = b.ws.clone()
        .or_else(|| q.ws.clone())
        .or_else(|| headers.get("x-agent-kb-workspace").and_then(|v| v.to_str().ok()).map(|x| x.to_string()))
        .unwrap_or_else(|| "default".into());
    s.store.ensure_workspace(&ws);
    let tags = b.tags.unwrap_or_default();
    let p = s.store.create_project(&ws, &b.name, &b.goal, b.status.as_deref().unwrap_or("active"), &tags);
    (StatusCode::CREATED, Json(json!({ "ok": true, "project": p }))).into_response()
}

async fn get_project(State(s): State<AppState>, Path(id): Path<String>, Query(q): Query<WsQuery>, headers: axum::http::HeaderMap) -> Response {
    let ws = pick_ws(&q, &headers);
    match s.store.get_project(&ws, &id) {
        Some(p) => Json(json!({ "ok": true, "project": p })).into_response(),
        None => err(StatusCode::NOT_FOUND, "项目不存在"),
    }
}

#[derive(Deserialize)]
struct ProjectPatch {
    name: Option<String>,
    goal: Option<String>,
    status: Option<String>,
    tags: Option<Vec<String>>,
}

async fn patch_project(State(s): State<AppState>, Path(id): Path<String>, Query(q): Query<WsQuery>, headers: axum::http::HeaderMap, Json(b): Json<ProjectPatch>) -> Response {
    let ws = pick_ws(&q, &headers);
    let ok = s.store.update_project(&ws, &id, b.name.as_deref(), b.goal.as_deref(), b.status.as_deref(), b.tags.as_deref());
    if ok { Json(json!({ "ok": true, "project": s.store.get_project(&ws, &id) })).into_response() }
    else { err(StatusCode::NOT_FOUND, "项目不存在") }
}

async fn delete_project(State(s): State<AppState>, Path(id): Path<String>, Query(q): Query<WsQuery>, headers: axum::http::HeaderMap) -> Response {
    let ws = pick_ws(&q, &headers);
    if s.store.delete_project(&ws, &id) { Json(json!({ "ok": true })).into_response() }
    else { err(StatusCode::NOT_FOUND, "项目不存在") }
}

#[derive(Deserialize)]
struct KVBody { #[serde(default)] label: String, value: String }
#[derive(Deserialize)]
struct TextBody { text: String }

async fn add_host(State(s): State<AppState>, Path(id): Path<String>, Query(q): Query<WsQuery>, headers: axum::http::HeaderMap, Json(b): Json<KVBody>) -> Response {
    let ws = pick_ws(&q, &headers);
    match s.store.append_json(&ws, &id, "hosts", json!({"label": b.label, "value": b.value})) {
        Some(a) => Json(json!({ "ok": true, "hosts": a })).into_response(),
        None => err(StatusCode::NOT_FOUND, "项目不存在"),
    }
}
async fn add_material(State(s): State<AppState>, Path(id): Path<String>, Query(q): Query<WsQuery>, headers: axum::http::HeaderMap, Json(b): Json<KVBody>) -> Response {
    let ws = pick_ws(&q, &headers);
    match s.store.append_json(&ws, &id, "materials", json!({"label": b.label, "value": b.value})) {
        Some(a) => Json(json!({ "ok": true, "materials": a })).into_response(),
        None => err(StatusCode::NOT_FOUND, "项目不存在"),
    }
}
async fn add_decision(State(s): State<AppState>, Path(id): Path<String>, Query(q): Query<WsQuery>, headers: axum::http::HeaderMap, Json(b): Json<TextBody>) -> Response {
    let ws = pick_ws(&q, &headers);
    let v = json!({"text": b.text, "at": chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string()});
    match s.store.append_json(&ws, &id, "decisions", v) {
        Some(a) => Json(json!({ "ok": true, "decisions": a })).into_response(),
        None => err(StatusCode::NOT_FOUND, "项目不存在"),
    }
}
async fn add_timeline(State(s): State<AppState>, Path(id): Path<String>, Query(q): Query<WsQuery>, headers: axum::http::HeaderMap, Json(b): Json<TextBody>) -> Response {
    let ws = pick_ws(&q, &headers);
    let v = json!({"text": b.text, "at": chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string()});
    match s.store.append_json(&ws, &id, "timeline", v) {
        Some(a) => Json(json!({ "ok": true, "timeline": a })).into_response(),
        None => err(StatusCode::NOT_FOUND, "项目不存在"),
    }
}

#[derive(Deserialize)]
struct NewTask {
    title: String,
    #[serde(default)] note: String,
    #[serde(default)] column: Option<String>,
    #[serde(default)] priority: Option<String>,
}
async fn add_task(State(s): State<AppState>, Path(id): Path<String>, Query(q): Query<WsQuery>, headers: axum::http::HeaderMap, Json(b): Json<NewTask>) -> Response {
    let ws = pick_ws(&q, &headers);
    match s.store.create_task(&ws, &id, &b.title, &b.note, b.column.as_deref().unwrap_or("todo"), b.priority.as_deref().unwrap_or("normal")) {
        Some(t) => (StatusCode::CREATED, Json(json!({ "ok": true, "task": t }))).into_response(),
        None => err(StatusCode::NOT_FOUND, "项目不存在"),
    }
}

#[derive(Deserialize)]
struct TaskPatchBody {
    title: Option<String>,
    note: Option<String>,
    column: Option<String>,
    order: Option<i64>,
    priority: Option<String>,
}
async fn patch_task(State(s): State<AppState>, Path((id, tid)): Path<(String, String)>, Query(q): Query<WsQuery>, headers: axum::http::HeaderMap, Json(b): Json<TaskPatchBody>) -> Response {
    let ws = pick_ws(&q, &headers);
    let patch = TaskPatch { title: b.title, note: b.note, column: b.column, order: b.order, priority: b.priority };
    match s.store.update_task(&ws, &id, &tid, &patch) {
        Some(t) => Json(json!({ "ok": true, "task": t })).into_response(),
        None => err(StatusCode::NOT_FOUND, "任务不存在"),
    }
}

async fn del_task(State(s): State<AppState>, Path((id, tid)): Path<(String, String)>, Query(q): Query<WsQuery>, headers: axum::http::HeaderMap) -> Response {
    let ws = pick_ws(&q, &headers);
    if s.store.delete_task(&ws, &id, &tid) { Json(json!({ "ok": true })).into_response() }
    else { err(StatusCode::NOT_FOUND, "任务不存在") }
}

#[derive(Deserialize)]
struct ReorderBody { column: String, ids: Vec<String> }
async fn reorder_tasks(State(s): State<AppState>, Path(id): Path<String>, Query(q): Query<WsQuery>, headers: axum::http::HeaderMap, Json(b): Json<ReorderBody>) -> Response {
    let ws = pick_ws(&q, &headers);
    if s.store.reorder_tasks(&ws, &id, &b.column, &b.ids) { Json(json!({ "ok": true })).into_response() }
    else { err(StatusCode::BAD_REQUEST, "重排失败") }
}
