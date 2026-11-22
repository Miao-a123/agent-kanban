//! Domain model. All structs serialize to camelCase JSON for the web UI + MCP.

use serde::{Deserialize, Serialize};

/// A workspace belongs to one agent. Everything (projects, tasks…) is scoped by workspace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub id: String,          // slug, unique
    pub name: String,        // display name
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub workspace: String,     // owning workspace id
    pub name: String,
    pub goal: String,
    pub status: String,        // active | planning | paused | done | archived
    pub tags: Vec<String>,
    #[serde(default)]
    pub hosts: Vec<KV>,
    #[serde(default)]
    pub materials: Vec<KV>,
    #[serde(default)]
    pub decisions: Vec<Decision>,
    #[serde(default)]
    pub timeline: Vec<Event>,
    #[serde(default)]
    pub tasks: Vec<Task>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KV {
    pub label: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Decision {
    pub text: String,
    pub at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub text: String,
    pub at: String,
}

/// A kanban task card. Column is one of: todo | doing | done.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub note: String,
    pub column: String,      // todo | doing | done
    pub order: i64,          // position within column (ascending)
    pub priority: String,    // low | normal | high
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskPatch {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub column: Option<String>,
    #[serde(default)]
    pub order: Option<i64>,
    #[serde(default)]
    pub priority: Option<String>,
}

/// Lightweight project listing row (no nested collections).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSummary {
    pub id: String,
    pub workspace: String,
    pub name: String,
    pub goal: String,
    pub status: String,
    pub tags: Vec<String>,
    pub task_count: i64,
    pub todo_count: i64,
    pub doing_count: i64,
    pub done_count: i64,
    pub created_at: String,
    pub updated_at: String,
}

pub const STATUSES: &[&str] = &["active", "planning", "paused", "done", "archived"];
pub const COLUMNS: &[&str] = &["todo", "doing", "done"];
pub const PRIORITIES: &[&str] = &["low", "normal", "high"];
