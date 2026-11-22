# Agent Kanban — 多 Agent 项目管理看板

一个用 **Rust** 写成的独立小应用：每个接入它的 MCP agent 都拥有**自己的项目管理看板区**，可在对话里直接建项目、排任务、拖列、维护，互不干扰；同时带一个网页看板（Kanban 三列，可拖拽）。

- 零外部依赖：`rusqlite` 用 `bundled` 特性，编译出一个**单文件二进制**（约 2.7 MB），拷到任意机器直接跑。
- 两种运行模式共用一个 `--db` 数据库文件：
  - `serve`：HTTP + 网页看板（给人和浏览器用）
  - `mcp`：MCP stdio 服务（给 agent 用）
- **隔离模型**：以「工作空间(workspace)」为界。一个 MCP 连接的 `--agent <名字>`（或环境变量 `AGENT_ID`，缺省取当前目录名）就是一个工作空间；agent A 看不见、也改不到 agent B 的数据。网页端可用 `?ws=<名字>` 或请求头 `X-Agent-Kb-Workspace` 切换查看不同 agent 的看板。

---

## 快速开始

```bash
cargo build --release
# 单二进制：target/release/agent-kanban(.exe)
```

### 1) 起网页看板

```bash
./agent-kanban --db agent-kanban.db serve --host 127.0.0.1 --port 8096
# 浏览器打开 http://127.0.0.1:8096/?ws=dsh-agent
```

### 2) 接进 MCP（给 agent 用）

```bash
./agent-kanban --db agent-kanban.db --agent <agent名> mcp
```

以 DSH 为例，在 profile 的 `cordis.patch.yml` 加：

```yaml
- insert:
    - id: mcp-agent-kanban
      name: '@deepseek-ai/dsh-mcp-client'
      config:
        serverName: agent-kanban
        transport: stdio
        command: '.../agent-kanban(.exe)'
        args: ['--db', '.../agent-kanban.db', '--agent', 'dsh-agent', 'mcp']
        toolCallTimeoutMs: 30000
        failOnStartupError: false
```

每个 agent 用不同的 `--agent` 名字，就各自拥有独立看板。

---

## MCP 工具（14 个）

| 工具 | 作用 |
| --- | --- |
| `ak_whoami` | 我是哪个工作空间 + 概览 |
| `ak_overview` | 本工作空间总项目/任务/按状态统计 |
| `ak_list_projects` | 列项目（可 q 搜索 / status / tag 过滤） |
| `ak_get_project` | 项目详情（含全部任务） |
| `ak_add_project` | 新建项目（name, goal, status, tags） |
| `ak_update_project` | 改项目字段 |
| `ak_add_host` | 加一条部署位置（label, value） |
| `ak_add_material` | 加一条材料/文件/链接 |
| `ak_add_decision` | 记一条重要决策/踩坑 |
| `ak_add_timeline` | 加一条时间线记录 |
| `ak_add_task` | 新建任务卡片（可给 project_id 或 project 名字） |
| `ak_move_task` | 移动任务到某列（todo/doing/done，可给 order） |
| `ak_update_task` | 改任务字段 |
| `ak_delete_task` | 删任务 |

> 任务类工具的 `project_id` 字段也可以直接传**项目名**（会自动解析），方便 agent 用自然语言操作。

## REST API

| 方法 | 路径 |
| --- | --- |
| GET | `/api/health`、`/api/workspaces`、`/api/overview` |
| GET/POST | `/api/projects`（`?ws=` 指定工作空间） |
| GET/PATCH/DELETE | `/api/projects/:id` |
| POST | `/api/projects/:id/{hosts,materials,decisions,events}` |
| POST | `/api/projects/:id/tasks`、`/api/projects/:id/tasks/reorder` |
| PATCH/DELETE | `/api/projects/:id/tasks/:tid` |

所有路径都可带 `?ws=<工作空间>`。

## 数据模型

- **workspace**：id(由名字 slug 化)、name、created_at
- **project**：id、workspace、name、goal、status(`active/planning/paused/done/archived`)、tags、hosts、materials、decisions、timeline、tasks
- **task**：id、project_id、title、note、column(`todo/doing/done`)、order、priority(`low/normal/high`)

存储：SQLite（WAL），`projects.tasks` 通过外键 `ON DELETE CASCADE`，删项目连带删任务。

---

## 开发备忘 / 踩坑

- **MSVC 链接器写临时文件失败**：报 `LNK1104 无法打开文件 ...lnk*.tmp (os error 5 拒绝访问)`。构建前必须把临时目录指到工程内可写目录：
  ```bash
  export TMP="<工程>/.tmp"; export TEMP="$TMP"
  ```
- **favicon 的 SVG 用 `concat!` 拼**：`r#"..."#` raw string 里含 `#fff` 会和 `#` 结束定界符冲突。
- **工作空间 id 会 slug 化**：`ensure_workspace("agentA")` → `"agenta"`。所以插入项目必须用 slug 后的 id（`create_project` 内部已 `ensure_workspace` 并把 workspace 落成 slug id），否则会 `FOREIGN KEY constraint failed`。
- 编译出的 exe 正在运行时 `cargo build` 无法覆盖 → 先杀进程再 build。

## 项目结构

```
agent-kanban/
  Cargo.toml
  src/
    main.rs      入口：clap 解析，serve / mcp 两种子命令，resolve_agent
    model.rs     数据模型 + 常量（STATUSES/COLUMNS/PRIORITIES）
    store.rs     SQLite 存储层（workspace 隔离、CRUD、reorder）
    mcp.rs       MCP stdio JSON-RPC（14 工具，以 workspace 为隔离边界）
    http.rs      axum HTTP + REST + 网页路由
    webui.rs     内嵌 web/index.html
  web/index.html 单文件 Kanban SPA（三列拖拽）
```
