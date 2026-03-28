# Telecodex Setup Guide

## Prerequisites

- Rust 1.85+ (install via `rustup`)
- A locally installed and working `codex` CLI
- A Telegram account

---

## Step 1: Create a Telegram Bot

1. Search for **@BotFather** on Telegram, tap Start
2. Send `/newbot`
3. Enter a display name for the bot (e.g. `My Codex Bot`)
4. Enter a username ending in `bot` (e.g. `mycodex_bot`)
5. BotFather will reply with a token like:
   ```
   123456789:ABCdefGHIjklMNOpqrSTUvwxYZ
   ```
   **Save this token — you will need it later.**

---

## Step 2: Get Your Telegram User ID

Search for **@userinfobot** on Telegram and send any message. It will reply with your numeric ID:

```
Id: 987654321
```

---

## Step 3: Build the Project

```bash
cd /path/to/telecodex

# Default build (no local ONNX needed — sufficient for most use cases)
cargo build --release
```

If you want local Parakeet speech-to-text (requires access to cdn.pyke.io for ONNX Runtime download):

```bash
cargo build --release --features local-transcription
```

---

## Step 4: Create the Configuration File

```bash
cp telecodex.toml.example telecodex.toml
```

Edit `telecodex.toml`. A minimal working configuration:

```toml
db_path = "telecodex.sqlite3"
startup_admin_ids = [987654321]       # Replace with your Telegram user ID
poll_timeout_seconds = 30
edit_debounce_ms = 900
max_text_chunk = 3500
tmp_dir = "/absolute/path/to/telecodex/tmp"  # Must be an absolute path

[telegram]
bot_token_env = "TELEGRAM_BOT_TOKEN"
api_base = "https://api.telegram.org"
use_message_drafts = true

[codex]
binary = "codex"
default_cwd = "/absolute/path/to/your/workspace"  # Default working directory for Codex
default_model = "gpt-4o"
default_reasoning_effort = "medium"
default_sandbox = "workspace-write"
default_approval = "never"
default_search_mode = "disabled"
import_desktop_history = true
import_cli_history = true
```

### Configuration Reference

| Field | Description |
|---|---|
| `startup_admin_ids` | Telegram user IDs that are automatically granted admin role on startup. The first ID receives start/stop notifications |
| `tmp_dir` | Temporary directory for attachments. Must exist and be an absolute path |
| `bot_token_env` | Name of the environment variable holding the bot token. Using an env var is recommended over inlining the token |
| `default_cwd` | Default working directory for Codex sessions |
| `default_sandbox` | Sandbox mode: `read-only` / `workspace-write` / `danger-full-access` |
| `default_approval` | Approval policy: `never` / `on-request` / `untrusted` |

---

## Step 5: Create the tmp Directory

```bash
mkdir -p /absolute/path/to/telecodex/tmp
```

---

## Step 6: Start the Bot

```bash
export TELEGRAM_BOT_TOKEN="123456789:ABCdefGHIjklMNOpqrSTUvwxYZ"
cargo run --release -- telecodex.toml
```

On successful startup, the bot sends a message to the first user in `startup_admin_ids`:

```
🟢 Telecodex v0.1.0 started
```

---

## Step 7: Log in to Codex

Choose one of the two login methods depending on your setup.

### Option A: ChatGPT Account Login (Subscription Mode)

Send this to your bot on Telegram:

```
/login
```

The bot will reply with an OpenAI device verification link and a one-time code:

```
1. Open: https://auth.openai.com/codex/device
2. Enter code: XXXX-XXXX
```

Complete authorization in the browser, and the bot will confirm login success.

---

### Option B: API Key Login (BYOK Mode)

Since Codex 0.35.0, setting `OPENAI_API_KEY` no longer auto-authenticates. You must explicitly log in from the terminal **before** starting Telecodex:

```bash
printenv OPENAI_API_KEY | codex login --with-api-key
```

Verify login:

```bash
codex login status
# Should print "Logged in..."
```

Once confirmed, start Telecodex — no need to send `/login` from Telegram.

> **Note**: `codex login --api-key <KEY>` is the legacy syntax. Since 0.35.0, use `--with-api-key` via stdin. The `printenv` pipe avoids exposing the key in shell history.

---

## Common Commands

| Command | Description |
|---|---|
| `/new` | Start a fresh Codex session in the current topic |
| `/sessions` | List all sessions in this chat |
| `/environments` | Show importable Codex environments (forum dashboard only) |
| `/stop` | Interrupt the active turn |
| `/model [model]` | Show or switch the current model |
| `/sandbox <mode>` | Change sandbox mode |
| `/cd <path>` | Change working directory |
| `/pwd` | Show current working directory |
| `/logout` | Log out of Codex |

See the full command list in the [README](README.md#-command-model).

---

## Network Notes (China)

If `api.telegram.org` is not directly accessible, set a proxy before starting:

```bash
export HTTPS_PROXY=http://127.0.0.1:7890
export HTTP_PROXY=http://127.0.0.1:7890
export TELEGRAM_BOT_TOKEN="your-token-here"
cargo run --release -- telecodex.toml
```

---

## Advanced: Forum Mode (Multi-Topic Workspaces)

To map each Telegram Forum Topic to an independent Codex workspace:

1. Create a Telegram **supergroup** and enable **Topics (Forum)** mode
2. Add the bot to the group and grant it admin permissions
3. Get the group chat ID (a negative number, e.g. `-1001234567890`)
4. Add to `telecodex.toml`:

```toml
[telegram]
primary_forum_chat_id = -1001234567890
auto_create_topics = false  # Set to true to auto-create a topic for each environment
```

Then send `/environments` in the forum dashboard (the group's root channel) to browse and import local Codex history environments.
