# dauclem

A fast, lightweight desktop application and local viewer to search, filter, and browse recent [Claude Code](https://docs.anthropic.com/en/docs/agents-and-tools/claude-code/overview) conversation history.

Built as a native **Tauri v2** desktop app with an embedded **Rust** core, plus an optional zero-dependency **Node.js** web server fallback.

---

## 🎯 Goal

Claude Code stores rich conversation sessions locally in JSONL format, but browsing past conversations, inspecting tool invocations, and locating past solutions across projects can be cumbersome.

**dauclem** provides an instant local search engine and viewer tailored specifically for Claude Code transcripts:
- **Instant Search**: Find keywords across user prompts, Claude's answers, code edits, tool calls, and subagent runs in milliseconds.
- **Native & Lightweight**: Built with Tauri v2 and native Rust. Tiny memory footprint (~30 MB RAM) with zero runtime dependencies.
- **Automatic Background Indexing**: Detects new and updated chats automatically without blocking the UI.
- **Rich Chat View**: Clean readable layout with Markdown rendering, file diff views, collapsible tool outputs, and deep links into subagents.

---

## ✨ Features

- **⚡ Fast SQLite FTS5 Full-Text Search**: Uses SQLite's FTS5 engine with trigram tokenization for instant substring search, snippet generation, and match highlighting.
- **🏷️ Category Filtering**: Filter search results by:
  - **You**: User prompts and messages
  - **Claude**: Assistant responses
  - **Actions**: Tool calls (`Bash`, `FileEdit`, `Glob`, etc.) and their outputs
  - **Subagents**: Forked and delegated subagent transcripts
- **📁 Project Filtering**: Group and filter conversations by project workspace or directory path.
- **🔄 Incremental Auto-Sync**: Background worker checks `~/.claude/projects/` periodically (every 20s) and prioritizes indexing newest chats first.
- **🎨 Interactive Viewer**:
  - Side-by-side diffs for file edits.
  - Collapsible tool executions and truncated output inspection.
  - Subagent navigation directly from parent session logs.
  - Live query highlighting inside open chats.
- **⌨️ Keyboard-Friendly**: Press `/` anywhere in the app to instantly focus the search bar; `Ctrl+F` to search within an active chat.

---

## 📋 Requirements

### For the Desktop App (Tauri v2)
- **Runtime**: None! The compiled `.exe` / installer is completely standalone.
- **Building from source**:
  - [Rust](https://www.rust-lang.org/tools/install) (1.78+)
  - Node.js (for `@tauri-apps/cli`)
  - Claude Code conversation logs located in `~/.claude/projects` (default location).

### For the Web Server (Optional Fallback)
- **Node.js >= 22.5.0** (required for built-in [`node:sqlite`](https://nodejs.org/api/sqlite.html) `DatabaseSync` support).
- No `npm install` needed for web mode.

---

## 🚀 Quick Start

### 🖥️ Option A: Native Desktop App (Recommended)

Run as a high-performance desktop app:

```bash
# 1. Install Tauri CLI (one-time)
npm install

# 2. Run in development mode (hot-reloading desktop window)
npm run dev

# 3. Build standalone production installer / executable
npm run build
```

The compiled release executable will be in `src-tauri/target/release/dauclem.exe` (or `src-tauri/target/debug/dauclem.exe` in debug mode).

### 🌐 Option B: Local Web Server

Run in your browser using the Node.js standard library:

```bash
# Start the web server (no npm install required!)
npm start
# or: node server.js
```

Then open your browser at:
```
http://localhost:4173
```

On initial launch, the indexer scans and indexes your existing Claude Code sessions into `data/index.db`. Recent sessions are indexed first and become searchable immediately.

---

## ⚙️ Configuration

You can configure the application using environment variables:

| Variable | Description | Default | Supported By |
| :--- | :--- | :--- | :--- |
| `CLAUDE_PROJECTS` | Custom path to Claude Code project directory | `~/.claude/projects` | Desktop App & Web Server |
| `PORT` | HTTP server port | `4173` | Web Server only |

### Examples

**Point to a custom Claude projects directory (Windows PowerShell):**
```powershell
$env:CLAUDE_PROJECTS="D:\custom\.claude\projects"
npm run dev   # or: npm start
```

**Point to a custom Claude projects directory (POSIX / bash):**
```bash
CLAUDE_PROJECTS="/path/to/custom/.claude/projects" npm run dev
```

---

## 🧪 Testing

Run the native Rust test suite (verifies SQLite FTS5 trigram indexing and transcript parsing):

```bash
cargo test --manifest-path src-tauri/Cargo.toml
```

---

## 📂 Project Structure

```text
dauclem/
├── src-tauri/                 # Native Rust backend (Tauri v2)
│   ├── Cargo.toml             # Rust dependencies (rusqlite with FTS5, tauri v2, serde)
│   ├── tauri.conf.json        # Window settings, permissions, bundle configs
│   ├── capabilities/          # Tauri v2 window permissions
│   ├── icons/                 # Cross-platform application icons
│   └── src/
│       ├── main.rs            # Desktop executable entry point
│       ├── lib.rs             # Tauri setup & background indexing loop (20s)
│       ├── db.rs              # SQLite schema, WAL mode, trigram FTS5 index
│       ├── parser.rs          # Streaming JSONL transcript parser & event normalizer
│       ├── indexer.rs         # Incremental file watcher & SQLite batch transaction sync
│       └── commands.rs        # Tauri IPC commands (status, projects, sessions, search, session_detail)
├── server.js                  # Standalone Node.js HTTP server (optional fallback)
├── src/                       # Node.js backend modules (optional fallback)
│   ├── db.js                  # SQLite database schema, FTS5 trigram index, and triggers
│   ├── indexer.js             # Session file scanning, hash tracking, and batch indexing
│   ├── parse.js               # Fast JSONL transcript parser & event normalizer
│   └── worker.js              # Worker thread entrypoint for background indexing
├── public/                    # Shared UI (used by both Tauri Desktop & Web Server)
│   ├── index.html             # Single-page interface layout
│   ├── app.js                 # Frontend controller (dual-mode: Tauri IPC invoke or fetch)
│   └── style.css              # Dark-themed responsive styling
├── data/
│   └── index.db               # SQLite database (git-ignored)
└── package.json               # Project manifest & dev scripts
```

---

## 🔌 API & IPC Commands

Both the Tauri IPC interface (`window.__TAURI__.core.invoke`) and the HTTP REST server share identical capabilities:

| Tauri IPC Command | HTTP Endpoint | Description |
| :--- | :--- | :--- |
| `status` | `GET /api/status` | Indexing state (`indexing`, `done`, `total`, `version`) |
| `projects` | `GET /api/projects` | List detected projects, session counts, latest timestamp |
| `sessions` | `GET /api/sessions?project=…` | List sessions (optionally filtered by project) |
| `search` | `GET /api/search?q=…&cats=…` | Full-text trigram search with highlighted snippets |
| `session_detail` | `GET /api/session/:id` | Full transcript, parsed tool events, and subagent list |
| `session_detail` | `GET /api/session/:id?agent=…` | Transcript for a specific subagent |

---

## 📄 License

MIT (or private/internal use).
