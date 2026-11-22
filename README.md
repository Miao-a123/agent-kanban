# Agent Kanban — 多 Agent 项目管理看板

一个用 **Rust** 写成的独立小应用：每个接入它的 MCP agent 都拥有**自己的项目管理看板区**，可在对话里直接建项目、写项目档案、排任务、拖列、维护，互不干扰；同时带一个网页工作台。

**核心定位：以「项目档案」为主，任务是附属。** 每个项目有 7 个信息区——项目介绍 / 功能特性 / 项目截图 / 部署地址 / 相关材料 / 修改日志 / 重要决策，任务只是其中的「任务看板」标签页。

- 零外部依赖：`rusqlite` 用 `bundled` 特性，编译出一个**单文件二进制**（约 2.8 MB），拷到任意机器直接跑。
- 两种运行模式共用一个 `--db` 数据库文件：
  - `serve`：HTTP + 网页工作台（给人和浏览器用）
  - `mcp`：MCP stdio 服务（给 agent 用）
- **隔离模型**：以「工作空间(workspace)」为界。一个 MCP 连接的 `--agent <名字>`（或环境变量 `AGENT_ID`，缺省取当前目录名）就是一个工作空间；agent A 看不见、也改不到 agent B 的数据。网页端可用 `?ws=<名字>` 或请求头 `X-Agent-Kb-Workspace` 切换查看不同 agent 的看板。

---

## 网页 UI 结构（三栏）

```
┌──────┬──────────────┬───────────────────────────────────┐
│ 图标 │  项目列表     │  主区（Tab：项目信息 / 任务看板）   │
│ 栏   │ 搜索+卡片     │  ┌ 项目介绍                        │
│  🗂   │  + 新建项目   │  ┌ 功能 / 特性                     │
│ DS   │              │  ┌ 项目截图（点击放大）             │
│ RE   │              │  ┌ 项目地址 / 部署位置              │
│ ...  │              │  ┌ 相关文件 / 材料 / 链接           │
│      │              │  ┌ 修改日志 / 时间线                │
│      │              │  ┌ 重要决策 / 踩坑                  │
└──────┴──────────────┴───────────────────────────────────┘
```

- **最左 agent 图标栏**：每个接入的 agent（工作空间）一个圆形头像，点一下即切换到该 agent 的看板，便于快速切换。
- **中间项目列表**：搜索 + 项目卡片（显示状态徽章、标签、待办/进行/完成计数）。
- **主区两个标签页**：
  - **项目信息**（默认）：7 个区块，每块都可内联添加内容。
  - **任务看板**：Kanban 三列（待办/进行中/完成），HTML5 拖拽换列。

---

## 快速开始

```bash
cargo build --release
# 单二进制：target/release/agent-kanban(.exe)
```

### 1) 起网页工作台

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
        command: '.../agent-kanban-bin.exe'
        args: ['--db', '.../agent-kanban.db', '--agent', 'dsh-agent', 'mcp']
        toolCallTimeoutMs: 30000
        failOnStartupError: false
```

每个 agent 用不同的 `--agent` 名字，就各自拥有独立看板。

> **小技巧**：把 `command` 指向一个**固定的版本化文件名**（如 `agent-kanban-bin.exe`）。每次发新版时先 `cp` 成这个文件名再重启 DSH，这样正在运行的旧进程占用的不是你要覆盖的那个文件，避免 `Device or resource busy` / `LNK1104`。

---

## MCP 工具（16 个）

| 工具 | 作用 |
| --- | --- |
| `ak_whoami` | 我是哪个工作空间 + 概览 |
| `ak_overview` | 本工作空间总项目/任务/按状态统计 |
| `ak_list_projects` | 列项目（可 q 搜索 / status / tag 过滤） |
| `ak_get_project` | 项目详情（含全部任务） |
| `ak_add_project` | 新建项目（name, goal, status, tags） |
| `ak_update_project` | 改项目字段（含 intro 介绍） |
| `ak_add_feature` | 加一条功能/特性 |
| `ak_add_screenshot` | 加一张项目截图 URL |
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
| POST | `/api/projects/:id/{hosts,materials,decisions,events,features,screenshots}` |
| POST | `/api/projects/:id/tasks`、`/api/projects/:id/tasks/reorder` |
| PATCH/DELETE | `/api/projects/:id/tasks/:tid` |

所有路径都可带 `?ws=<工作空间>`。

## 数据模型

- **workspace**：id(由名字 slug 化)、name、created_at
- **project**：id、workspace、name、goal、**intro(介绍)**、**features(功能列表)**、**screenshots(截图URL列表)**、status(`active/planning/paused/done/archived`)、tags、hosts、materials、decisions、timeline、tasks
- **task**：id、project_id、title、note、column(`todo/doing/done`)、order、priority(`low/normal/high`)

存储：SQLite（WAL），`projects.tasks` 通过外键 `ON DELETE CASCADE`，删项目连带删任务。老库升级：`migrate()` 里用 `ALTER TABLE projects ADD COLUMN intro/features/screenshots ...`（`let _ =` 忽略「已存在」错误），平滑兼容旧数据。

---

## 开发备忘 / 踩坑

- **MSVC 链接器写临时文件失败**：报 `LNK1104 无法打开文件 ...lnk*.tmp (os error 5 拒绝访问)`。构建前必须把临时目录指到工程内可写目录：
  ```bash
  export TMP="<工程>/.tmp"; export TEMP="$TMP"
  ```
- **exe 被占用时构建失败**：编译出的 exe 正在运行（尤其被 DSH 的 MCP 子进程持有）时，`cargo build --release` 会报 `failed to remove file target/release/agent-kanban.exe`。两个办法：
  1. 用独立 target 目录：`CARGO_TARGET_DIR=target2 cargo build --release`；
  2. 或让 profile 指向版本化文件名（见上文「小技巧」），先杀旧进程再覆盖。
- **favicon 的 SVG 用 `concat!` 拼**：`r#"..."#` raw string 里含 `#fff` 会和 `#` 结束定界符冲突。
- **工作空间 id 会 slug 化**：`ensure_workspace("agentA")` → `"agenta"`。所以插入项目必须用 slug 后的 id（`create_project` 内部已 `ensure_workspace` 并把 workspace 落成 slug id），否则会 `FOREIGN KEY constraint failed`。
- **`create_project` 的 workspace 解析**：优先级 `body.ws > ?ws= > 请求头 > "default"`，早先只读 body/header 导致 `?ws=` 被忽略（项目误落 default），已修。
- **中文路径 + schtasks 不可靠**：启动持久服务建议用 bash 后台 `&` 保活，或 Windows「启动」文件夹。

## 项目结构

```
agent-kanban/
  Cargo.toml
  src/
    main.rs      入口：clap 解析，serve / mcp 两种子命令，resolve_agent
    model.rs     数据模型 + 常量（STATUSES/COLUMNS/PRIORITIES）
    store.rs     SQLite 存储层（workspace 隔离、CRUD、reorder、intro/features/screenshots）
    mcp.rs       MCP stdio JSON-RPC（16 工具，以 workspace 为隔离边界）
    http.rs      axum HTTP + REST + 网页路由
    webui.rs     内嵌 web/index.html
  web/index.html 单文件三栏 SPA（agent 图标栏 + 项目列表 + 信息/看板 Tab）
```
