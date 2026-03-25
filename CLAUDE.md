# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**Telecodex** is a Rust bridge that exposes a local `codex` CLI as a Telegram bot. Telegram chats/topics become workspaces; the bot relays messages to Codex and streams responses back by editing Telegram messages in-place. No webhook infrastructure — uses long polling only.

## Commands

```bash
# Build
task build             # debug
task build-release     # release

# Run (config defaults to telecodex.toml)
task run
task run CONFIG=telecodex.toml

# Tests
task test              # cargo test
cargo test <name>      # run a single test

# Quality
task verify            # fmt-check + test + clippy (full suite)
task fmt               # format
task check             # cargo check
task clippy            # cargo clippy --all-targets --all-features -- -D warnings

# Initial setup
task init-config       # copies telecodex.toml.example → telecodex.toml
```

Environment variable `TELEGRAM_BOT_TOKEN` must be set before running. Config path is the first CLI argument.

## Architecture

### Core execution flow

```
Telegram long-poll (getUpdates)
  → process_update() [app.rs]
    → dispatch_command_text() → handle_command() → enqueue_turn()
    → [non-command text/attachments] → enqueue_turn()
      → worker_for(session_key) — per-session tokio task with mpsc channel
        → process_turn() [app/turns.rs]
          → send Telegram placeholder message
          → CodexRunner::run_turn() [codex.rs]
          → LiveTurnSink edits placeholder in real time
          → send_generated_artifacts() uploads output files
```

### Codex integration mechanism (`src/codex.rs`)

There are two modes:

1. **`app-server` mode (normal turns)**: Spawns `codex app-server` as a subprocess. Communicates via JSON-RPC over stdin/stdout (newline-delimited JSON). Flow:
   - `initialize` handshake
   - `thread/start` or `thread/resume` (keyed by stored `codex_thread_id`)
   - `turn/start` with prompt + attachments as input
   - Listen to notifications: `item/agentMessage/delta`, `turn/completed`, `item/started`, etc.
   - Handle `ServerRequest` messages for approval callbacks (`item/commandExecution/requestApproval`, `item/fileChange/requestApproval`)

2. **Review mode**: Spawns `codex exec review --json` and parses JSON event lines from stdout. Triggered by `/review` command.

### Session model (`src/store.rs`)

Sessions are keyed by `SessionKey { chat_id, thread_id }`. Each session stores:
- `cwd` — workspace directory
- `codex_thread_id` — bound Codex thread UUID (auto-resolved from history on first use)
- `force_fresh_thread` — cleared by `/new`, forces a new Codex conversation
- Per-session settings: model, reasoning_effort, session_prompt, sandbox_mode, approval_policy, search_mode, add_dirs

The store is SQLite (`rusqlite`, WAL mode). The `workers` map in `App` holds one `SessionWorkerHandle` per active session (tokio `mpsc` + `CancellationToken`).

### Key files

| File | Responsibility |
|---|---|
| `src/app.rs` | Main runtime loop, update dispatch, command handling, session lifecycle |
| `src/app/turns.rs` | `process_turn()`, `LiveTurnSink` (real-time Telegram edits), artifact upload |
| `src/codex.rs` | `CodexRunner` — spawns Codex subprocess, JSON-RPC protocol, event parsing |
| `src/codex_history.rs` | Reads local Codex history files to import sessions and resolve thread IDs |
| `src/store.rs` | SQLite persistence for sessions, users, ACL, audit log |
| `src/telegram.rs` | Telegram Bot API HTTP client (reqwest) |
| `src/commands.rs` | Command parsing; `BridgeCommand` vs `ParsedInput::Forward` routing |
| `src/app/forum.rs` | Forum topic creation and sync with Codex Desktop/CLI history |
| `src/app/auth.rs` | `/login` device-code flow, `/logout` |
| `src/app/io.rs` | Attachment download from Telegram, file staging into `.telecodex/inbox/` |
| `src/config.rs` | TOML config loading and validation |
| `src/transcribe.rs` | Optional audio transcription via `transcribe-rs` + Handy Parakeet model |

### File paths used at runtime

```
<session cwd>/.telecodex/inbox/           # staged incoming attachments
<session cwd>/.telecodex/turns/<id>/out/  # expected output artifacts (auto-uploaded)
```

### Command routing

Commands are either:
- **Bridge-handled** (`BridgeCommand`): managed entirely in Rust (session control, ACL, settings)
- **Forwarded** (`ParsedInput::Forward`): passed as prompt text to Codex (e.g., `/help`, `/status`)
- **Unsupported**: rejected with a message (TUI-only commands)

### Access control

SQLite `users` table with `allowed` flag and `role` (`admin`/`user`). Unauthorized updates are silently dropped and written to `audit_log`. `startup_admin_ids` in config seeds initial admins on startup.
