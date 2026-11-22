//! Minimal stdio JSON-RPC 2.0 MCP server (protocol 2024-11-05).
//! Each connection = one agent workspace (identified by AGENT_ID env or --agent / cwd).

use crate::model::*;
use crate::store::Store;
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::sync::Arc;

pub fn run(store: Arc<Store>, workspace: String) {
    store.ensure_workspace(&workspace);
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = match line { Ok(l) => l, Err(_) => break };
        let line = line.trim();
        if line.is_empty() { continue; }
        let req: Value = match serde_json::from_str(line) { Ok(v) => v, Err(_) => continue };
        // notifications (no id) -> ignore (initialized etc.)
        let idv = req.get("id").cloned();
        let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let params = req.get("params").cloned().unwrap_or(Value::Null);

        // notifications have no id
        let resp = match (&idv, method) {
            (_, "initialize") => Some(result(idv, json!({
                "protocolVersion": "2024-11-05",
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "agent-kanban", "version": env!("CARGO_PKG_VERSION") },
            }))),
            (_, "tools/list") => Some(result(idv, json!({ "tools": tool_defs() }))),
            (_, "tools/call") => Some(result(idv, call_tool(&store, &workspace, &params))),
            (_, "ping") => Some(result(idv, json!({}))),
            (Some(_), unknown) => Some(error(idv.clone(), -32601, &format!("unknown method: {}", unknown))),
            (None, _) => None,
        };
        if let Some(r) = resp {
            let _ = writeln!(stdout, "{}", serde_json::to_string(&r).unwrap());
            let _ = stdout.flush();
        }
    }
}

fn result(id: Option<Value>, v: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id.unwrap_or(Value::Null), "result": v })
}

fn error(id: Option<Value>, code: i64, msg: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id.unwrap_or(Value::Null), "error": { "code": code, "message": msg } })
}

fn text_result(v: Value) -> Value {
    json!({ "content": [ { "type": "text", "text": serde_json::to_string_pretty(&v).unwrap() } ], "isError": false })
}

fn err_result(msg: &str) -> Value {
    json!({ "content": [ { "type": "text", "text": msg } ], "isError": true })
}

fn tool_defs() -> Value {
    json!([
        { "name": "ak_whoami", "description": "显示当前 agent 的工作空间 id/名称，以及其中的项目数。",
          "inputSchema": { "type": "object", "properties": {} } },
        { "name": "ak_overview", "description": "当前 agent 工作空间的概览：项目数、各状态计数、任务数。",
          "inputSchema": { "type": "object", "properties": {} } },
        { "name": "ak_list_projects", "description": "列出【本 agent】的项目（看板）。可选 q/status/tag 过滤。",
          "inputSchema": { "type": "object", "properties": {
            "q": {"type":"string"}, "status": {"type":"string"}, "tag": {"type":"string"} } } },
        { "name": "ak_get_project", "description": "取单个项目完整详情：部署位置、材料、决策、时间线、以及全部任务卡片。",
          "inputSchema": { "type": "object", "properties": { "id": {"type":"string"} }, "required":["id"] } },
        { "name": "ak_add_project", "description": "新建一个项目（即一块新看板）。",
          "inputSchema": { "type": "object", "properties": {
            "name": {"type":"string"}, "goal": {"type":"string"},
            "status": {"type":"string","description":"active|planning|paused|done|archived"},
            "tags": {"type":"array","items":{"type":"string"}} }, "required":["name"] } },
        { "name": "ak_update_project", "description": "更新项目字段（name/goal/status/tags/intro 介绍）。",
          "inputSchema": { "type": "object", "properties": {
            "id": {"type":"string"}, "name": {"type":"string"}, "goal": {"type":"string"},
            "status": {"type":"string"}, "tags": {"type":"array","items":{"type":"string"}},
            "intro": {"type":"string","description":"项目介绍（可较长，纯文本/条列）"} }, "required":["id"] } },
        { "name": "ak_add_feature", "description": "给项目追加一条功能点（功能清单）。",
          "inputSchema": { "type": "object", "properties": {
            "id": {"type":"string"}, "feature": {"type":"string"} }, "required":["id","feature"] } },
        { "name": "ak_add_screenshot", "description": "给项目追加一张截图（URL 或本地路径）。",
          "inputSchema": { "type": "object", "properties": {
            "id": {"type":"string"}, "url": {"type":"string"} }, "required":["id","url"] } },
        { "name": "ak_add_host", "description": "给项目追加一条部署位置/地址/路径。",
          "inputSchema": { "type": "object", "properties": {
            "id": {"type":"string"}, "label": {"type":"string"}, "value": {"type":"string"} }, "required":["id","value"] } },
        { "name": "ak_add_material", "description": "给项目追加一条关键材料/文件/链接。",
          "inputSchema": { "type": "object", "properties": {
            "id": {"type":"string"}, "label": {"type":"string"}, "value": {"type":"string"} }, "required":["id","value"] } },
        { "name": "ak_add_decision", "description": "给项目追加一条重要决策/踩坑。",
          "inputSchema": { "type": "object", "properties": {
            "id": {"type":"string"}, "text": {"type":"string"} }, "required":["id","text"] } },
        { "name": "ak_add_timeline", "description": "给项目追加一条时间线记录。",
          "inputSchema": { "type": "object", "properties": {
            "id": {"type":"string"}, "text": {"type":"string"} }, "required":["id","text"] } },
        { "name": "ak_add_task", "description": "在看板上新建一张任务卡片（默认在 todo 列）。",
          "inputSchema": { "type": "object", "properties": {
            "project_id": {"type":"string"}, "title": {"type":"string"},
            "note": {"type":"string"}, "column": {"type":"string","description":"todo|doing|done"},
            "priority": {"type":"string","description":"low|normal|high"} }, "required":["project_id","title"] } },
        { "name": "ak_move_task", "description": "移动任务卡片到某列（可给 order 指定列内顺序）。",
          "inputSchema": { "type": "object", "properties": {
            "project_id": {"type":"string"}, "task_id": {"type":"string"},
            "column": {"type":"string","description":"todo|doing|done"}, "order": {"type":"integer"} },
          "required":["project_id","task_id","column"] } },
        { "name": "ak_update_task", "description": "更新任务卡片字段（title/note/priority/column）。",
          "inputSchema": { "type": "object", "properties": {
            "project_id": {"type":"string"}, "task_id": {"type":"string"},
            "title": {"type":"string"}, "note": {"type":"string"},
            "priority": {"type":"string"}, "column": {"type":"string"} }, "required":["project_id","task_id"] } },
        { "name": "ak_delete_task", "description": "删除一张任务卡片。",
          "inputSchema": { "type": "object", "properties": {
            "project_id": {"type":"string"}, "task_id": {"type":"string"} }, "required":["project_id","task_id"] } },
    ])
}

fn s(v: &Value, k: &str) -> String { v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string() }

/// 取项目 id：优先 project_id，其次 project（可用项目名），解析成该 workspace 下的真实 id。
fn pid_of(store: &Store, ws: &str, args: &Value) -> Result<String, String> {
    let raw = if !s(args, "project_id").is_empty() { s(args, "project_id") }
              else if !s(args, "project").is_empty() { s(args, "project") }
              else { return Err("缺少 project_id（可用项目 id 或项目名 project）".into()); };
    store.resolve_pid(ws, &raw).ok_or_else(|| format!("项目不存在或不属于当前 agent：{}", raw))
}

fn call_tool(store: &Store, ws: &str, params: &Value) -> Value {
    let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
    let args = params.get("arguments").cloned().unwrap_or(json!({}));
    match name {
        "ak_whoami" => {
            let ov = store.workspace_overview(ws);
            text_result(json!({ "workspace": ws, "overview": ov }))
        }
        "ak_overview" => text_result(store.workspace_overview(ws)),
        "ak_list_projects" => {
            let q = args.get("q").and_then(|x| x.as_str());
            let st = args.get("status").and_then(|x| x.as_str());
            let tg = args.get("tag").and_then(|x| x.as_str());
            let items = store.list_projects(ws, q, st, tg);
            text_result(json!({ "count": items.len(), "items": items }))
        }
        "ak_get_project" => {
            let id = s(&args, "id");
            match store.get_project(ws, &id) {
                Some(p) => text_result(json!(p)),
                None => err_result("项目不存在或不属于当前 agent"),
            }
        }
        "ak_add_project" => {
            let tags: Vec<String> = args.get("tags").and_then(|t| t.as_array())
                .map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect())
                .unwrap_or_default();
            let status = if args.get("status").is_some() { s(&args, "status") } else { "active".to_string() };
            let p = store.create_project(ws, &s(&args, "name"), &s(&args, "goal"), &status, &tags);
            text_result(json!({ "ok": true, "id": p.id, "project": p }))
        }
        "ak_update_project" => {
            let id = s(&args, "id");
            let tags: Option<Vec<String>> = args.get("tags").and_then(|t| t.as_array())
                .map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect());
            let ok = store.update_project(ws, &id,
                args.get("name").and_then(|x| x.as_str()),
                args.get("goal").and_then(|x| x.as_str()),
                args.get("status").and_then(|x| x.as_str()),
                tags.as_deref(),
                args.get("intro").and_then(|x| x.as_str()));
            if ok { text_result(json!({ "ok": true, "project": store.get_project(ws, &id) })) }
            else { err_result("项目不存在或不属于当前 agent") }
        }
        "ak_add_host" => {
            let id = s(&args, "id");
            let v = json!({ "label": s(&args,"label"), "value": s(&args,"value") });
            match store.append_json(ws, &id, "hosts", v) { Some(a) => text_result(json!({ "ok": true, "hosts": a })), None => err_result("项目不存在或不属于当前 agent") }
        }
        "ak_add_material" => {
            let id = s(&args, "id");
            let v = json!({ "label": s(&args,"label"), "value": s(&args,"value") });
            match store.append_json(ws, &id, "materials", v) { Some(a) => text_result(json!({ "ok": true, "materials": a })), None => err_result("项目不存在或不属于当前 agent") }
        }
        "ak_add_decision" => {
            let id = s(&args, "id");
            let v = json!({ "text": s(&args,"text"), "at": chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string() });
            match store.append_json(ws, &id, "decisions", v) { Some(a) => text_result(json!({ "ok": true, "decisions": a })), None => err_result("项目不存在或不属于当前 agent") }
        }
        "ak_add_timeline" => {
            let id = s(&args, "id");
            let v = json!({ "text": s(&args,"text"), "at": chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string() });
            match store.append_json(ws, &id, "timeline", v) { Some(a) => text_result(json!({ "ok": true, "timeline": a })), None => err_result("项目不存在或不属于当前 agent") }
        }
        "ak_add_feature" => {
            let id = s(&args, "id");
            match store.append_json(ws, &id, "features", json!(s(&args, "feature"))) { Some(a) => text_result(json!({ "ok": true, "features": a })), None => err_result("项目不存在或不属于当前 agent") }
        }
        "ak_add_screenshot" => {
            let id = s(&args, "id");
            match store.append_json(ws, &id, "screenshots", json!(s(&args, "url"))) { Some(a) => text_result(json!({ "ok": true, "screenshots": a })), None => err_result("项目不存在或不属于当前 agent") }
        }
        "ak_add_task" => {
            let pid = match pid_of(store, ws, &args) {
                Ok(v) => v,
                Err(e) => return err_result(&e),
            };
            let col = if args.get("column").is_some() { s(&args,"column") } else { "todo".into() };
            let pr = if args.get("priority").is_some() { s(&args,"priority") } else { "normal".into() };
            match store.create_task(ws, &pid, &s(&args,"title"), &s(&args,"note"), &col, &pr) {
                Some(t) => text_result(json!({ "ok": true, "task": t })),
                None => err_result("项目不存在或不属于当前 agent"),
            }
        }
        "ak_move_task" => {
            let pid = match pid_of(store, ws, &args) {
                Ok(v) => v,
                Err(e) => return err_result(&e),
            };
            let tid = s(&args, "task_id");
            let patch = TaskPatch {
                title: None, note: None,
                column: Some(s(&args, "column")),
                order: args.get("order").and_then(|x| x.as_i64()),
                priority: None,
            };
            match store.update_task(ws, &pid, &tid, &patch) {
                Some(t) => text_result(json!({ "ok": true, "task": t })),
                None => err_result("任务不存在或不属于当前 agent"),
            }
        }
        "ak_update_task" => {
            let pid = match pid_of(store, ws, &args) {
                Ok(v) => v,
                Err(e) => return err_result(&e),
            };
            let tid = s(&args, "task_id");
            let patch = TaskPatch {
                title: args.get("title").and_then(|x| x.as_str()).map(|s| s.to_string()),
                note: args.get("note").and_then(|x| x.as_str()).map(|s| s.to_string()),
                column: args.get("column").and_then(|x| x.as_str()).map(|s| s.to_string()),
                order: args.get("order").and_then(|x| x.as_i64()),
                priority: args.get("priority").and_then(|x| x.as_str()).map(|s| s.to_string()),
            };
            match store.update_task(ws, &pid, &tid, &patch) {
                Some(t) => text_result(json!({ "ok": true, "task": t })),
                None => err_result("任务不存在或不属于当前 agent"),
            }
        }
        "ak_delete_task" => {
            let pid = match pid_of(store, ws, &args) {
                Ok(v) => v,
                Err(e) => return err_result(&e),
            };
            let tid = s(&args, "task_id");
            if store.delete_task(ws, &pid, &tid) { text_result(json!({ "ok": true })) }
            else { err_result("任务不存在或不属于当前 agent") }
        }
        _ => err_result(&format!("未知工具: {}", name)),
    }
}
