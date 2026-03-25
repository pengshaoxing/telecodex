# 2026-03-25 流式消息与日志改进计划

## 问题概述

### 问题 1：Codex 回复在 Telegram 中"卡 typing"或内容丢失

**现象**：发送消息后 Telegram 一直显示 typing，最终超时；或完成后只能看到部分内容。

**根因**：`LiveTurnSink::flush(force=false)` 在流式更新时调用 `truncate_for_live_update()`，只取第一个 chunk（≤3500 字符），通过 `editMessageText` 编辑同一条消息。当 Codex 输出超过 3500 字符时：

1. 超出部分被静默丢弃，用户只看到前面截断的内容
2. 截断可能发生在 markdown 代码块中间，导致 `render_markdown_to_html` 生成不完整的 HTML
3. Telegram 拒绝格式错误的 HTML，`editMessageText` 返回 400 错误
4. 进入 fallback 后仍可能超长失败
5. 碰到 rate limit 时 `edit_backoff_until` 期间所有更新被静默跳过

只有 `finish(force=true)` 时才会调用 `split_text` 拆成多条消息发送，但此时用户已经等了很久看不到中间过程。

**影响文件**：
- `src/app/turns.rs` — `LiveTurnSink::flush()` 方法
- `src/render.rs` — `split_text()` 和 `render_markdown_to_html()`

### 问题 2：终端无法观察 Codex 的回复内容和事件流

**现象**：进程启动后只有一条启动日志，Codex 的回复内容、turn 生命周期、Telegram 编辑状态等关键信息完全不可见。

**根因**：

1. 默认日志级别 `telecodex=info`，但大部分 Codex 相关日志使用 `tracing::debug!`
2. `item/agentMessage/delta`、`item/completed`、`turn/completed` 等核心事件处理中**完全没有 tracing 输出**
3. `LiveTurnSink` 的 flush/edit 操作没有成功日志，只有错误时才有 warn

**影响文件**：
- `src/codex.rs` — 事件通知处理函数
- `src/app/turns.rs` — `process_turn()`、`LiveTurnSink`

---

## 改动计划

### 改动 A：流式消息多条拆分（`src/app/turns.rs`）

#### A1. `flush(force=false)` 改为支持多条消息

**位置**：`LiveTurnSink::flush()` 方法（约 L424-L485）

**当前逻辑**：
```rust
let chunks = if force {
    split_text(&visible_text, self.shared.config.max_text_chunk)
} else {
    vec![truncate_for_live_update(&visible_text, self.shared.config.max_text_chunk)]
};
```

**改为**：非 force 模式也使用 `split_text` 进行分片，但限制最大消息数避免刷屏。

```rust
let chunks = split_text(&visible_text, self.shared.config.max_text_chunk);
// 非 force 模式下限制最大消息数，避免流式过程中发太多消息
let chunks = if !force && chunks.len() > self.messages.len() + 1 {
    // 流式过程中最多比当前消息数多1条，其余截断到最后一条
    let mut limited = chunks[..self.messages.len()].to_vec();
    let remainder: String = chunks[self.messages.len()..].join("");
    let last_chunk = truncate_for_live_update(&remainder, self.shared.config.max_text_chunk);
    limited.push(last_chunk);
    limited
} else {
    chunks
};
```

核心思路：流式过程中逐步增加消息数（每次 flush 最多新增 1 条消息），而不是一次性全部拆分。这样既能展示长内容，又不会因为内容还在增长而频繁创建/删除消息。

#### A2. 清理多余消息

**位置**：`flush()` 方法末尾，chunks 循环之后

当 finish 时 chunks 数可能少于之前流式创建的消息数（比如最终文本变短了），需要删除多余的消息。

```rust
// finish 时清理多余消息
if force {
    while self.messages.len() > chunks.len() {
        if let Some(extra) = self.messages.pop() {
            let _ = self.shared.telegram
                .delete_message(extra.chat_id, extra.message_id)
                .await;
        }
    }
}
```

#### A3. 移除 `truncate_for_live_update` 的唯一调用

该函数仅在 flush 中使用，改动后仅作为内部 helper 保留（用于 A1 中的 remainder 截断），不再作为主路径。

---

### 改动 B：增加关键事件日志（`src/codex.rs` + `src/app/turns.rs`）

#### B1. Codex 事件通知日志（`src/codex.rs`）

在 `run_app_server_turn` 的事件分发处增加 info 级别日志：

| 事件 | 位置 | 日志内容 |
|------|------|---------|
| `turn/completed` | ~L461 | `info!("codex turn completed: status={status}")` |
| `item/agentMessage/delta` | ~L481 | `debug!("codex delta: {}chars total", summary.assistant_text.len())` （保持 debug 避免刷屏） |
| `item/started` (commandExecution) | ~L487 | `info!("codex executing command: {command}")` |
| `item/completed` (agentMessage) | ~L501 | `info!("codex message complete: {}chars", text.len())` |
| `item/completed` (commandExecution) | ~L510 | `info!("codex command {status}: {command}")` |
| ServerRequest (approval) | handle_server_request | `info!("codex requesting approval: {method}")` |
| ServerRequest (userInput) | handle_server_request | `info!("codex requesting user input")` |

#### B2. Turn 生命周期日志（`src/app/turns.rs`）

| 位置 | 日志内容 |
|------|---------|
| `process_turn` 开头 | `info!("turn started for {:?}", session.key)` |
| `process_turn` run_result Ok | `info!("turn completed for {:?}", session.key)` |
| `process_turn` run_result Err | （已有 warn，保持不变） |

#### B3. LiveTurnSink flush 日志（`src/app/turns.rs`）

| 位置 | 日志内容 |
|------|---------|
| flush 发送新消息时 | `debug!("new telegram message #{} for {:?}", idx, self.session_key)` |
| flush 编辑消息成功 | `debug!("edited message #{} for {:?}: {}chars", idx, self.session_key, chunk.len())` |
| flush 编辑触发 backoff | （已有 warn，保持不变） |

---

## 改动文件清单

| 文件 | 改动类型 | 内容 |
|------|---------|------|
| `src/app/turns.rs` | 修改 | A1: flush 多条拆分；A2: 多余消息清理；A3: truncate helper 调整；B2+B3: 日志 |
| `src/codex.rs` | 修改 | B1: 事件日志 |
| `src/telegram.rs` | 可能修改 | 如果 `delete_message` 方法不存在，需要新增 |

---

### 改动 C：内置日志文件输出（`src/main.rs` + `src/config.rs`）

**问题**：tracing 默认只输出到 stderr，进程退出后日志丢失。shell 重定向 `2>&1 | tee` 因 cargo 缓冲等原因不可靠。

**修复**：
- `src/config.rs`：`Config` 新增 `log_file: Option<PathBuf>`
- `src/main.rs`：重构初始化顺序，先加载 config 再初始化 tracing。使用 `tracing_subscriber::registry()` + 双层 Layer（stderr + 文件），文件层关闭 ANSI 颜色，append 模式写入。
- `telecodex.toml.example`：新增 `log_file = "telecodex.log"` 示例

---

### 改动 D：修复 Codex 子进程 shutdown 阻塞（`src/codex.rs`）— 关键 Bug

**问题**：第一个 turn 完成后，后续所有命令永久卡住。Telegram 持续显示 typing，终端无任何输出。

**现象**（来自 `telecodex.log`）：
```
04:48:44 INFO codex turn completed: status=completed, text=394chars
(此后无任何日志 — 没有 "turn completed for SessionKey...")
04:49:37 DEBUG received 1 update(s)
04:49:37 DEBUG checking codex auth status...
04:49:37 DEBUG codex auth: authenticated=true
(永久卡住)
```

**根因**：`AppServerProcess::shutdown()` 永久阻塞。

1. `terminate_child()` 中 `child.kill()` 后调用 `child.wait().await`，但 stdout pipe 仍由 `self.stdout_lines` 持有，fd 未关闭，`wait()` 在部分 Unix 环境下不返回
2. `self.stderr_task.await` 等待 stderr 读取循环 EOF，但如果 codex 子进程 fork 了孙进程（如执行 shell 命令），孙进程继承了 stderr fd，pipe 永远不关闭

因为 `process_turn` 卡在 `process.shutdown()`，worker channel 的 `rx.recv()` 永远不被再次调用，后续所有 turn 被永久排队。同时 `cancel.cancel()` 没被调用，typing indicator 持续发送。

**修复**：
- `shutdown()` 中先 `drop(self.stdin)` + `drop(self.stdout_lines)` 关闭 pipe fd
- `terminate_child()` 中 `child.wait()` 加 5 秒超时
- `stderr_task.await` 加 5 秒超时
- review 模式中的 `child.wait()` 和 `stderr_task.await` 同样加超时
- 增加诊断日志：`shutting down codex app-server process...` / `codex app-server shutdown complete`

**影响文件**：
- `src/codex.rs` — `AppServerProcess::shutdown()`、`terminate_child()`、`run_review_turn()`

### 改动 E：`ensure_codex_authenticated` 超时保护（`src/app/auth.rs`）

**问题**：`codex login status` 如果因网络或进程问题卡住，整个 polling loop 被阻塞。

**修复**：给 `auth_status()` 调用加 10 秒超时，超时后假定已认证，不阻塞后续消息处理。增加 debug 级别诊断日志。

---

## 验证方法

1. `cargo check` — 编译通过，0 warning
2. `cargo test` — 75 passed（1 个已有的 macOS 路径测试除外）
3. 用 `RUST_LOG=telecodex=debug` 启动，发送 `/help`，观察日志完整流程：
   - `turn started` → `codex delta` → `codex message complete` → `codex turn completed` → `shutting down` → `shutdown complete` → `turn completed for SessionKey`
4. 发送第二条消息 `/status`，确认能正常处理
5. 发送长回复触发消息，确认流式多消息拆分
6. 确认 `telecodex.log` 文件正常写入

---

## 已完成状态

| 改动 | 状态 | 日期 |
|------|------|------|
| A: 流式多条拆分 | ✅ 已完成 | 2026-03-25 |
| B: 关键事件日志 | ✅ 已完成 | 2026-03-25 |
| C: 内置日志文件 | ✅ 已完成 | 2026-03-25 |
| D: shutdown 阻塞修复 | ✅ 已完成 | 2026-03-25 |
| E: auth_status 超时保护 | ✅ 已完成 | 2026-03-25 |
