# Telecodex 配置与启动指南

## 前置条件

- Rust 1.85+（`rustup` 安装）
- 本地已安装并可用的 `codex` CLI
- Telegram 账号

---

## 第一步：创建 Telegram Bot

1. 在 Telegram 搜索 **@BotFather**，点击 Start
2. 发送 `/newbot`
3. 输入 bot 显示名称（如 `My Codex Bot`）
4. 输入 bot 用户名（必须以 `bot` 结尾，如 `mycodex_bot`）
5. BotFather 返回 token，格式类似：
   ```
   123456789:ABCdefGHIjklMNOpqrSTUvwxYZ
   ```
   **保存好这个 token，后续配置需要用到。**

---

## 第二步：获取你的 Telegram 用户 ID

在 Telegram 搜索 **@userinfobot**，发送任意消息，它会回复你的数字 ID：

```
Id: 987654321
```

---

## 第三步：编译项目

```bash
cd /path/to/telecodex

# 默认构建（不需要本地 ONNX，适合大多数场景）
cargo build --release
```

如果需要本地 Parakeet 语音转录（需要能访问 cdn.pyke.io 下载 ONNX Runtime）：

```bash
cargo build --release --features local-transcription
```

---

## 第四步：创建配置文件

```bash
cp telecodex.toml.example telecodex.toml
```

编辑 `telecodex.toml`，最简可用配置如下：

```toml
db_path = "telecodex.sqlite3"
startup_admin_ids = [987654321]       # 替换为你的 Telegram 用户 ID
poll_timeout_seconds = 30
edit_debounce_ms = 900
max_text_chunk = 3500
tmp_dir = "/absolute/path/to/telecodex/tmp"  # 必须是绝对路径

[telegram]
bot_token_env = "TELEGRAM_BOT_TOKEN"
api_base = "https://api.telegram.org"
use_message_drafts = true

[codex]
binary = "codex"
default_cwd = "/absolute/path/to/your/workspace"  # Codex 默认工作目录
default_model = "gpt-4o"
default_reasoning_effort = "medium"
default_sandbox = "workspace-write"
default_approval = "never"
default_search_mode = "disabled"
import_desktop_history = true
import_cli_history = true
```

### 配置说明

| 字段 | 说明 |
|---|---|
| `startup_admin_ids` | 启动时自动设为管理员的 Telegram 用户 ID 列表，第一个 ID 会收到启动/停止通知 |
| `tmp_dir` | 附件临时目录，必须提前创建，必须是绝对路径 |
| `bot_token_env` | 读取 bot token 的环境变量名，推荐用环境变量而非直接写 token |
| `default_cwd` | Codex 会话的默认工作目录 |
| `default_sandbox` | 沙箱模式：`read-only` / `workspace-write` / `danger-full-access` |
| `default_approval` | 审批策略：`never` / `on-request` / `untrusted` |

---

## 第五步：创建 tmp 目录

```bash
mkdir -p /absolute/path/to/telecodex/tmp
```

---

## 第六步：启动 Bot

```bash
export TELEGRAM_BOT_TOKEN="123456789:ABCdefGHIjklMNOpqrSTUvwxYZ"
cargo run --release -- telecodex.toml
```

启动成功后，bot 会向 `startup_admin_ids` 中第一个用户发送：

```
🟢 Telecodex v0.1.0 started
```

---

## 第七步：登录 Codex

有两种登录方式，根据你的使用模式选择其一。

### 方式一：ChatGPT 账号登录（订阅模式）

在 Telegram 中给你的 bot 发送：

```
/login
```

Bot 会发送一个 OpenAI 设备验证链接 + 一次性码：

```
1. 打开链接：https://auth.openai.com/codex/device
2. 输入一次性码：XXXX-XXXX
```

在浏览器中完成授权后，bot 会回复登录成功消息。

---

### 方式二：API Key 登录（BYOK 模式）

Codex 0.35.0 之后，设置 `OPENAI_API_KEY` 环境变量**不再**自动生效，必须在**启动 Telecodex 之前**在终端显式执行一次登录：

```bash
printenv OPENAI_API_KEY | codex login --with-api-key
```

验证登录是否成功：

```bash
codex login status
# 返回 "Logged in..." 表示成功
```

确认后再启动 Telecodex，无需在 Telegram 中发送 `/login`。

> **注意**：`codex login --api-key <KEY>` 是旧版参数写法，0.35.0+ 已迁移为 `--with-api-key` 并通过 stdin 传入，推荐用 `printenv` 管道方式避免 key 出现在 shell 历史记录中。

---

## 常用命令

| 命令 | 说明 |
|---|---|
| `/new` | 在当前 topic 开始新的 Codex 会话 |
| `/sessions` | 查看当前 chat 的所有会话 |
| `/environments` | 查看可导入的 Codex 环境（仅论坛 dashboard） |
| `/stop` | 中断当前正在执行的 turn |
| `/model [model]` | 查看或切换模型 |
| `/sandbox <mode>` | 切换沙箱模式 |
| `/cd <path>` | 切换工作目录 |
| `/pwd` | 查看当前工作目录 |
| `/logout` | 登出 Codex |

完整命令列表见 [README](../README.md#-command-model)。

---

## 国内网络注意事项

如果 `api.telegram.org` 无法直接访问，启动前设置代理：

```bash
export HTTPS_PROXY=http://127.0.0.1:7890
export HTTP_PROXY=http://127.0.0.1:7890
export TELEGRAM_BOT_TOKEN="your-token-here"
cargo run --release -- telecodex.toml
```

---

## 进阶：Forum 模式（多 Topic 工作区）

如果想让每个 Telegram Forum Topic 对应一个独立的 Codex 工作区：

1. 创建一个 Telegram **超级群组**，开启 **Forum（话题）** 功能
2. 将 bot 加入群组并设为管理员
3. 获取群组 chat ID（负数，如 `-1001234567890`）
4. 在 `telecodex.toml` 中添加：

```toml
[telegram]
primary_forum_chat_id = -1001234567890
auto_create_topics = false  # true 则自动为每个环境创建 topic
```

之后在 forum dashboard（群组根频道）发送 `/environments` 可查看并导入本地 Codex 历史环境。
