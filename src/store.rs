//! SQLite storage. One connection guarded by a Mutex; all access scoped by workspace.

use crate::model::*;
use rusqlite::{params, Connection, OptionalExtension};
use std::sync::Mutex;

pub struct Store {
    conn: Mutex<Connection>,
}

fn now() -> String {
    chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string()
}

fn new_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..12].to_string()
}

pub fn slug(s: &str) -> String {
    let mut out = String::new();
    for ch in s.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let t = out.trim_matches('-').to_string();
    if t.is_empty() { "agent".to_string() } else { t }
}

impl Store {
    pub fn open(path: &str) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let s = Store { conn: Mutex::new(conn) };
        s.migrate()?;
        Ok(s)
    }

    fn migrate(&self) -> rusqlite::Result<()> {
        let c = self.conn.lock().unwrap();
        c.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS workspaces (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS projects (
                id TEXT PRIMARY KEY,
                workspace TEXT NOT NULL REFERENCES workspaces(id),
                name TEXT NOT NULL,
                goal TEXT NOT NULL DEFAULT '',
                status TEXT NOT NULL DEFAULT 'active',
                tags TEXT NOT NULL DEFAULT '',
                hosts TEXT NOT NULL DEFAULT '[]',
                materials TEXT NOT NULL DEFAULT '[]',
                decisions TEXT NOT NULL DEFAULT '[]',
                timeline TEXT NOT NULL DEFAULT '[]',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_projects_ws ON projects(workspace);
            CREATE TABLE IF NOT EXISTS tasks (
                id TEXT PRIMARY KEY,
                project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                title TEXT NOT NULL,
                note TEXT NOT NULL DEFAULT '',
                col TEXT NOT NULL DEFAULT 'todo',
                ord INTEGER NOT NULL DEFAULT 0,
                priority TEXT NOT NULL DEFAULT 'normal',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_tasks_project ON tasks(project_id);
            "#,
        )?;
        Ok(())
    }

    // ---- workspaces ----
    pub fn ensure_workspace(&self, raw: &str) -> String {
        let id = slug(raw);
        let c = self.conn.lock().unwrap();
        let exists: Option<String> = c
            .query_row("SELECT id FROM workspaces WHERE id=?1", params![id], |r| r.get(0))
            .optional().unwrap();
        if exists.is_none() {
            let _ = c.execute(
                "INSERT INTO workspaces(id,name,created_at) VALUES(?1,?2,?3)",
                params![id, raw.trim(), now()],
            );
        }
        id
    }

    pub fn list_workspaces(&self) -> Vec<Workspace> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare("SELECT id,name,created_at FROM workspaces ORDER BY created_at").unwrap();
        let rows = st.query_map([], |r| {
            Ok(Workspace { id: r.get(0)?, name: r.get(1)?, created_at: r.get(2)? })
        }).unwrap();
        rows.filter_map(|x| x.ok()).collect()
    }

    // ---- projects ----
    pub fn create_project(&self, ws: &str, name: &str, goal: &str, status: &str, tags: &[String]) -> Project {
        let id = new_id();
        let ts = now();
        let status = if STATUSES.contains(&status) { status.to_string() } else { "active".into() };
        let ws = self.ensure_workspace(ws);
        let c = self.conn.lock().unwrap();
        c.execute(
            "INSERT INTO projects(id,workspace,name,goal,status,tags,created_at,updated_at) \
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![id, ws, name, goal, status, tags.join(","), ts, ts],
        ).unwrap();
        Project {
            id, workspace: ws.to_string(), name: name.to_string(), goal: goal.to_string(),
            status, tags: tags.to_vec(), hosts: vec![], materials: vec![], decisions: vec![],
            timeline: vec![], tasks: vec![], created_at: ts.clone(), updated_at: ts,
        }
    }

    fn row_to_project(r: &rusqlite::Row) -> rusqlite::Result<Project> {
        let tags: String = r.get("tags")?;
        let hosts: String = r.get("hosts")?;
        let materials: String = r.get("materials")?;
        let decisions: String = r.get("decisions")?;
        let timeline: String = r.get("timeline")?;
        Ok(Project {
            id: r.get("id")?,
            workspace: r.get("workspace")?,
            name: r.get("name")?,
            goal: r.get("goal")?,
            status: r.get("status")?,
            tags: if tags.is_empty() { vec![] } else { tags.split(',').map(|s| s.to_string()).collect() },
            hosts: serde_json::from_str(&hosts).unwrap_or_default(),
            materials: serde_json::from_str(&materials).unwrap_or_default(),
            decisions: serde_json::from_str(&decisions).unwrap_or_default(),
            timeline: serde_json::from_str(&timeline).unwrap_or_default(),
            tasks: vec![],
            created_at: r.get("created_at")?,
            updated_at: r.get("updated_at")?,
        })
    }

    /// 把「项目 id 或项目名」解析为该 workspace 下的项目 id。
    pub fn resolve_pid(&self, ws: &str, id_or_name: &str) -> Option<String> {
        let c = self.conn.lock().unwrap();
        let by_id: Option<String> = c
            .query_row("SELECT id FROM projects WHERE id=?1 AND workspace=?2",
                       params![id_or_name, ws], |r| r.get(0)).optional().ok().flatten();
        if by_id.is_some() { return by_id; }
        c.query_row("SELECT id FROM projects WHERE name=?1 AND workspace=?2 ORDER BY created_at LIMIT 1",
                    params![id_or_name, ws], |r| r.get(0)).optional().ok().flatten()
    }

    pub fn get_project(&self, ws: &str, id: &str) -> Option<Project> {
        let c = self.conn.lock().unwrap();
        let mut p: Project = c.query_row(
            "SELECT * FROM projects WHERE id=?1 AND workspace=?2",
            params![id, ws], Store::row_to_project,
        ).optional().ok()??;
        p.tasks = self.tasks_of(&c, id);
        Some(p)
    }

    pub fn list_projects(&self, ws: &str, q: Option<&str>, status: Option<&str>, tag: Option<&str>) -> Vec<ProjectSummary> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare(
            "SELECT id,workspace,name,goal,status,tags,created_at,updated_at FROM projects \
             WHERE workspace=?1 ORDER BY updated_at DESC",
        ).unwrap();
        let rows = st.query_map(params![ws], |r| {
            let tags: String = r.get("tags")?;
            Ok(ProjectSummary {
                id: r.get("id")?, workspace: r.get("workspace")?, name: r.get("name")?,
                goal: r.get("goal")?, status: r.get("status")?,
                tags: if tags.is_empty() { vec![] } else { tags.split(',').map(|s| s.to_string()).collect() },
                task_count: 0, todo_count: 0, doing_count: 0, done_count: 0,
                created_at: r.get("created_at")?, updated_at: r.get("updated_at")?,
            })
        }).unwrap();
        let mut out: Vec<ProjectSummary> = rows.filter_map(|x| x.ok()).collect();
        for p in out.iter_mut() {
            let (t, td, dg, dn) = self.counts_of(&c, &p.id);
            p.task_count = t; p.todo_count = td; p.doing_count = dg; p.done_count = dn;
        }
        // filter in-memory (small scale)
        let ql = q.map(|s| s.to_lowercase());
        out.retain(|p| {
            if let Some(st) = status { if p.status != st { return false; } }
            if let Some(t) = tag { if !p.tags.iter().any(|x| x == t) { return false; } }
            if let Some(ref ql) = ql {
                let hay = format!("{} {} {}", p.name, p.goal, p.tags.join(" ")).to_lowercase();
                if !hay.contains(ql.as_str()) { return false; }
            }
            true
        });
        out
    }

    fn counts_of(&self, c: &Connection, pid: &str) -> (i64, i64, i64, i64) {
        let mut st = c.prepare("SELECT col, COUNT(*) FROM tasks WHERE project_id=?1 GROUP BY col").unwrap();
        let rows = st.query_map(params![pid], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))).unwrap();
        let (mut t, mut td, mut dg, mut dn) = (0, 0, 0, 0);
        for row in rows.flatten() {
            t += row.1;
            match row.0.as_str() { "todo" => td = row.1, "doing" => dg = row.1, "done" => dn = row.1, _ => {} }
        }
        (t, td, dg, dn)
    }

    pub fn update_project(&self, ws: &str, id: &str, name: Option<&str>, goal: Option<&str>, status: Option<&str>, tags: Option<&[String]>) -> bool {
        let c = self.conn.lock().unwrap();
        let ts = now();
        let mut sets: Vec<String> = vec!["updated_at=?1".into()];
        // build dynamic update
        let mut vals: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(ts)];
        if let Some(n) = name { vals.push(Box::new(n.to_string())); sets.push(format!("name=?{}", vals.len())); }
        if let Some(g) = goal { vals.push(Box::new(g.to_string())); sets.push(format!("goal=?{}", vals.len())); }
        if let Some(s) = status {
            if STATUSES.contains(&s) { vals.push(Box::new(s.to_string())); sets.push(format!("status=?{}", vals.len())); }
        }
        if let Some(t) = tags { vals.push(Box::new(t.join(","))); sets.push(format!("tags=?{}", vals.len())); }
        vals.push(Box::new(id.to_string()));
        let idpos = vals.len();
        vals.push(Box::new(ws.to_string()));
        let wspos = vals.len();
        let sql = format!("UPDATE projects SET {} WHERE id=?{} AND workspace=?{}", sets.join(","), idpos, wspos);
        let params_ref: Vec<&dyn rusqlite::ToSql> = vals.iter().map(|b| b.as_ref()).collect();
        c.execute(&sql, params_ref.as_slice()).unwrap_or(0) > 0
    }

    pub fn delete_project(&self, ws: &str, id: &str) -> bool {
        let c = self.conn.lock().unwrap();
        c.execute("DELETE FROM projects WHERE id=?1 AND workspace=?2", params![id, ws]).unwrap_or(0) > 0
    }

    /// Append to a JSON array column (hosts / materials / decisions / timeline).
    pub fn append_json(&self, ws: &str, id: &str, col: &str, val: serde_json::Value) -> Option<serde_json::Value> {
        let c = self.conn.lock().unwrap();
        let cur: String = c.query_row(
            &format!("SELECT {} FROM projects WHERE id=?1 AND workspace=?2", col),
            params![id, ws], |r| r.get(0),
        ).optional().ok()??;
        let mut arr: Vec<serde_json::Value> = serde_json::from_str(&cur).unwrap_or_default();
        arr.push(val);
        let newv = serde_json::to_string(&arr).unwrap();
        c.execute(
            &format!("UPDATE projects SET {}=?1, updated_at=?2 WHERE id=?3 AND workspace=?4", col),
            params![newv, now(), id, ws],
        ).ok()?;
        Some(serde_json::to_value(&arr).unwrap())
    }

    // ---- tasks ----
    fn row_to_task(r: &rusqlite::Row) -> rusqlite::Result<Task> {
        Ok(Task {
            id: r.get("id")?, project_id: r.get("project_id")?, title: r.get("title")?,
            note: r.get("note")?, column: r.get("col")?, order: r.get("ord")?,
            priority: r.get("priority")?, created_at: r.get("created_at")?, updated_at: r.get("updated_at")?,
        })
    }

    fn tasks_of(&self, c: &Connection, pid: &str) -> Vec<Task> {
        let mut st = c.prepare(
            "SELECT * FROM tasks WHERE project_id=?1 ORDER BY \
             CASE col WHEN 'todo' THEN 0 WHEN 'doing' THEN 1 ELSE 2 END, ord, created_at",
        ).unwrap();
        let rows = st.query_map(params![pid], Store::row_to_task).unwrap();
        rows.filter_map(|x| x.ok()).collect()
    }

    fn assert_owns(&self, c: &Connection, ws: &str, pid: &str) -> bool {
        c.query_row("SELECT 1 FROM projects WHERE id=?1 AND workspace=?2", params![pid, ws], |_| Ok(()))
            .optional().ok().flatten().is_some()
    }

    pub fn create_task(&self, ws: &str, pid: &str, title: &str, note: &str, column: &str, priority: &str) -> Option<Task> {
        let c = self.conn.lock().unwrap();
        if !self.assert_owns(&c, ws, pid) { return None; }
        let col = if COLUMNS.contains(&column) { column.to_string() } else { "todo".into() };
        let pr = if PRIORITIES.contains(&priority) { priority.to_string() } else { "normal".into() };
        let maxord: i64 = c.query_row(
            "SELECT COALESCE(MAX(ord),-1) FROM tasks WHERE project_id=?1 AND col=?2",
            params![pid, col], |r| r.get(0),
        ).unwrap_or(-1);
        let id = new_id();
        let ts = now();
        c.execute(
            "INSERT INTO tasks(id,project_id,title,note,col,ord,priority,created_at,updated_at) \
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![id, pid, title, note, col, maxord + 1, pr, ts, ts],
        ).unwrap();
        Some(Task {
            id, project_id: pid.to_string(), title: title.to_string(), note: note.to_string(),
            column: col, order: maxord + 1, priority: pr, created_at: ts.clone(), updated_at: ts,
        })
    }

    pub fn update_task(&self, ws: &str, pid: &str, tid: &str, p: &TaskPatch) -> Option<Task> {
        let c = self.conn.lock().unwrap();
        if !self.assert_owns(&c, ws, pid) { return None; }
        let ts = now();
        let mut sets: Vec<String> = vec!["updated_at=?1".into()];
        let mut vals: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(ts)];
        if let Some(t) = &p.title { vals.push(Box::new(t.clone())); sets.push(format!("title=?{}", vals.len())); }
        if let Some(n) = &p.note { vals.push(Box::new(n.clone())); sets.push(format!("note=?{}", vals.len())); }
        if let Some(col) = &p.column {
            if COLUMNS.contains(&col.as_str()) {
                vals.push(Box::new(col.clone()));
                sets.push(format!("col=?{}", vals.len()));
            }
        }
        if let Some(o) = p.order { vals.push(Box::new(o)); sets.push(format!("ord=?{}", vals.len())); }
        if let Some(pr) = &p.priority {
            if PRIORITIES.contains(&pr.as_str()) { vals.push(Box::new(pr.clone())); sets.push(format!("priority=?{}", vals.len())); }
        }
        vals.push(Box::new(tid.to_string())); let tpos = vals.len();
        vals.push(Box::new(pid.to_string())); let ppos = vals.len();
        let sql = format!("UPDATE tasks SET {} WHERE id=?{} AND project_id=?{}", sets.join(","), tpos, ppos);
        let params_ref: Vec<&dyn rusqlite::ToSql> = vals.iter().map(|b| b.as_ref()).collect();
        if c.execute(&sql, params_ref.as_slice()).unwrap_or(0) == 0 { return None; }
        c.query_row("SELECT * FROM tasks WHERE id=?1", params![tid], Store::row_to_task).optional().ok().flatten()
    }

    pub fn delete_task(&self, ws: &str, pid: &str, tid: &str) -> bool {
        let c = self.conn.lock().unwrap();
        if !self.assert_owns(&c, ws, pid) { return false; }
        c.execute("DELETE FROM tasks WHERE id=?1 AND project_id=?2", params![tid, pid]).unwrap_or(0) > 0
    }

    /// Reorder: given ordered list of task ids in a target column, reassign ord.
    pub fn reorder_tasks(&self, ws: &str, pid: &str, column: &str, ordered_ids: &[String]) -> bool {
        let c = self.conn.lock().unwrap();
        if !self.assert_owns(&c, ws, pid) { return false; }
        if !COLUMNS.contains(&column) { return false; }
        let ts = now();
        for (i, tid) in ordered_ids.iter().enumerate() {
            let _ = c.execute(
                "UPDATE tasks SET col=?1, ord=?2, updated_at=?3 WHERE id=?4 AND project_id=?5",
                params![column, i as i64, ts, tid, pid],
            );
        }
        true
    }

    pub fn workspace_overview(&self, ws: &str) -> serde_json::Value {
        let c = self.conn.lock().unwrap();
        let total: i64 = c.query_row("SELECT COUNT(*) FROM projects WHERE workspace=?1", params![ws], |r| r.get(0)).unwrap_or(0);
        let mut st = c.prepare("SELECT status, COUNT(*) FROM projects WHERE workspace=?1 GROUP BY status").unwrap();
        let rows = st.query_map(params![ws], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))).unwrap();
        let mut by = serde_json::Map::new();
        for row in rows.flatten() { by.insert(row.0, serde_json::json!(row.1)); }
        let tasks: i64 = c.query_row(
            "SELECT COUNT(*) FROM tasks t JOIN projects p ON t.project_id=p.id WHERE p.workspace=?1",
            params![ws], |r| r.get(0),
        ).unwrap_or(0);
        serde_json::json!({ "workspace": ws, "total_projects": total, "by_status": by, "total_tasks": tasks })
    }
}
