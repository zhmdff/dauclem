# dauclem

A fast, lightweight, zero-dependency local web application to search, filter, and browse recent [Claude Code](https://docs.anthropic.com/en/docs/agents-and-tools/claude-code/overview) conversation history.

---

## 🎯 Goal

Claude Code stores rich conversation sessions locally in JSONL format, but browsing past conversations, inspecting tool invocations, and locating past solutions across projects can be cumbersome.

**dauclem** provides a fast local search engine and web viewer tailored specifically for Claude Code transcripts:
- **Instant Search**: Find keywords across user prompts, Claude's answers, code edits, tool calls, and subagent runs in milliseconds.
- **Zero NPM Dependencies**: Built entirely using the Node.js standard library (Node.js 22.5+ native SQLite support). No `node_modules` footprint.
- **Automatic Background Indexing**: Detects new and updated chats automatically without blocking the UI.
- **Rich Chat View**: Clean readable layout with Markdown rendering, file diff views, collapsible tool outputs, and deep links into subagents.

---

## ✨ Features

- **⚡ Fast SQLite FTS5 Full-Text Search**: Uses SQLite's FTS5 engine with trigram tokenization for instant search results, snippet generation, and match highlighting.
- **🏷️ Category Filtering**: Filter search results by:
  - **You**: User prompts and messages
  - **Claude**: Assistant responses
  - **Actions**: Tool calls (`Bash`, `FileEdit`, `Glob`, etc.) and their outputs
  - **Subagents**: Forked and delegated subagent transcripts
- **📁 Project Filtering**: Group and filter conversations by project workspace or directory path.
- **🔄 Incremental Auto-Sync**: A background worker thread checks `~/.claude/projects/` periodically (every 20s) and prioritizes indexing newest chats first.
- **🎨 Interactive Viewer**:
  - Side-by-side diffs for file edits.
  - Collapsible tool executions and truncated output inspection.
  - Subagent navigation directly from parent session logs.
  - Live query highlighting inside open chats.
- **⌨️ Keyboard-Friendly**: Press `/` anywhere in the app to instantly focus the search bar.

---

## 📋 Requirements

- **Node.js >= 22.5.0** (required for built-in [`node:sqlite`](https://nodejs.org/api/sqlite.html) `DatabaseSync` support).
- Claude Code conversation logs located in `~/.claude/projects` (default location).

---

## 🚀 Quick Start

1. **Clone or navigate to the repository:**
   ```bash
   cd dauclem
   ```

2. **Start the server** (no `npm install` needed!):
   ```bash
   npm start
   # or: node server.js
   ```

3. **Open your browser:**
   ```
   http://localhost:4173
   ```

On initial launch, the background worker will scan and index your existing Claude Code sessions into `data/index.db`. Recent sessions are indexed first and become searchable immediately.

---

## ⚙️ Configuration

You can configure the server using environment variables:

| Variable | Description | Default |
| :--- | :--- | :--- |
| `PORT` | HTTP server port | `4173` |
| `CLAUDE_PROJECTS` | Custom path to Claude Code project directory | `~/.claude/projects` |

### Examples

**Run on a custom port:**
```bash
PORT=5000 npm start
```

**Point to a custom Claude projects directory (Windows PowerShell):**
```powershell
$env:CLAUDE_PROJECTS="D:\custom\.claude\projects"
npm start
```

**Point to a custom Claude projects directory (POSIX / bash):**
```bash
CLAUDE_PROJECTS="/path/to/custom/.claude/projects" npm start
```

---

## 📂 Project Structure

```text
dauclem/
├── server.js          # HTTP server, static file server, and REST API endpoints
├── src/
│   ├── db.js          # SQLite database schema, FTS5 trigram index, and triggers
│   ├── indexer.js     # Session file scanning, hash tracking, and batch indexing
│   ├── parse.js       # Fast JSONL transcript parser & event normalizer
│   └── worker.js      # Worker thread entrypoint for non-blocking background indexing
├── public/
│   ├── index.html     # Single-page interface layout
│   ├── app.js         # Frontend controller, diff generator, and search renderer
│   └── style.css      # Dark-themed responsive styling
├── data/
│   └── index.db       # Generated SQLite database (git-ignored)
└── package.json       # Project manifest
```

---

## 🔌 API Endpoints

The built-in HTTP server exposes lightweight JSON endpoints:

- `GET /api/status`: Returns indexing state (`indexing`, `done`, `total`, `error`).
- `GET /api/projects`: List all detected projects, their session counts, and latest activity timestamp.
- `GET /api/sessions?project=<name>`: List sessions (optionally filtered by project).
- `GET /api/search?q=<query>&cats=<me,agent,act,sub>&project=<name>&sort=<rank|date>&limit=<n>`: Full-text search with highlighted snippets.
- `GET /api/session/<id>`: Retrieve parsed items and subagents for a session.
- `GET /api/session/<id>?agent=<subagentId>`: Retrieve items for a specific subagent.

---

## 📄 License

MIT (or private/internal use).
