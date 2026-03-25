# 持久化 AppServerProcess：跨 Turn 复用 Codex 子进程

## Context

当前每次 turn 都 spawn 一个新的 `codex app-server` 子进程（spawn → initialize → thread/start → event loop → shutdown），下次 turn 又重新来一遍。实际上 `codex app-server` 是长驻设计，支持在同一进程内执行多轮 turn。每次重启浪费 ~200ms 启动 + 初始化开销，且无法利用进程内缓存。

## 改动文件（3 个）

```
src/codex.rs        — 新增 SessionAppServer 包装，拆分 run_app_server_turn
src/app.rs          — Worker loop 持有 server 状态，新增 WorkerMessage 枚举
src/app/turns.rs    — process_turn 签名变更，接收 &mut Option<SessionAppServer>
```

---

## Step 1: `src/codex.rs` — 核心重构

### 1a. 新增 `SessionAppServer` 结构体

```rust
pub(crate) struct SessionAppServer {
    process: AppServerProcess,
}
```

将 `AppServerProcess` 的可见性改为 `pub(crate)`（当前是 private）。

### 1b. `SessionAppServer` 方法

```rust
impl SessionAppServer {
    /// 创建新进程：spawn + initialize + start_or_resume_thread
    async fn new(binary: &Path, session: &SessionRecord, request: &TurnRequest) -> Result<Self>;

    /// 检查子进程是否存活（try_wait）
    fn is_alive(&mut self) -> bool;

    /// 每轮 turn 开始前调用：如果进程死了则重启，然后 start_or_resume_thread
    async fn ensure_ready(&mut self, binary: &Path, session: &SessionRecord, request: &TurnRequest) -> Result<String>;

    /// 消费并关闭进程
    async fn shutdown(self) -> Result<String>;
}
```

### 1c. 新增 `run_turn_on_process` — 从 `run_app_server_turn` 中提取事件循环

```rust
async fn run_turn_on_process<F, Fut>(
    process: &mut AppServerProcess,
    thread_id: &str,
    session: &SessionRecord,
    request: &TurnRequest,
    cancel: CancellationToken,
    on_event: &mut F,
) -> Result<(RunSummary, bool)>  // bool = 进程仍可用
```

包含当前 L388-418 的逻辑（turn/start → event pump → turn/completed），但：
- **不调用 shutdown**
- 返回 `(summary, process_usable)` — 正常完成/成功中断时 `true`，超时/崩溃时 `false`

### 1d. 新增 `CodexRunner::run_turn_with_server`

```rust
pub async fn run_turn_with_server<F, Fut>(
    &self,
    server: &mut Option<SessionAppServer>,
    session: &SessionRecord,
    request: &TurnRequest,
    cancel: CancellationToken,
    on_event: F,
) -> Result<RunSummary>
```

逻辑：
1. 如果 `server.is_none()` → `SessionAppServer::new()` 创建
2. 如果 `server.is_some()` → `server.ensure_ready()` 检查存活 + resume thread
3. 调用 `run_turn_on_process()`
4. 如果返回 `process_usable=false` → `server.take().shutdown()`
5. Review mode 路径不变（仍用独立进程）

### 1e. 保留旧 `run_turn` 作为兼容

旧 `run_turn()` 保持不变（用于没有 server 的场景和 review mode），`run_turn_with_server` 是新的主路径。

---

## Step 2: `src/app.rs` — Worker 生命周期管理

### 2a. 新增 `WorkerMessage` 枚举

```rust
enum WorkerMessage {
    Turn(QueuedTurn),
    InvalidateProcess,  // 通知 worker 丢弃当前进程
}
```

### 2b. `SessionWorkerHandle.sender` 类型变更

```rust
sender: mpsc::UnboundedSender<WorkerMessage>,
```

`enqueue_turn()` 改为发送 `WorkerMessage::Turn(queued)`。

### 2c. Worker loop 持有本地 server 状态

```rust
tokio::spawn(async move {
    let mut server: Option<SessionAppServer> = None;
    loop {
        match tokio::time::timeout(Duration::from_secs(300), rx.recv()).await {
            Ok(Some(WorkerMessage::Turn(turn))) => {
                if let Err(error) = process_turn(
                    shared.clone(), cancel.clone(), &mut server, turn
                ).await {
                    tracing::error!("turn failed for {:?}: {error:#}", key);
                }
            }
            Ok(Some(WorkerMessage::InvalidateProcess)) => {
                if let Some(s) = server.take() {
                    tracing::info!("invalidating app-server for {:?}", key);
                    let _ = s.shutdown().await;
                }
            }
            Ok(None) => break,  // channel closed
            Err(_) => {
                // 5 分钟空闲超时：关闭进程释放资源，worker 继续等待
                if let Some(s) = server.take() {
                    tracing::info!("idle timeout: shutting down app-server for {:?}", key);
                    let _ = s.shutdown().await;
                }
            }
        }
    }
    // Worker 退出时清理
    if let Some(s) = server.take() {
        let _ = s.shutdown().await;
    }
});
```

### 2d. 新增 `invalidate_session_process` 方法

```rust
async fn invalidate_session_process(&self, key: SessionKey) {
    if let Some(handle) = self.workers.lock().await.get(&key).cloned() {
        let _ = handle.sender.send(WorkerMessage::InvalidateProcess);
    }
}
```

在以下位置调用：
- `BridgeCommand::New` — 新建会话
- `BridgeCommand::Clear` — 清除会话
- `BridgeCommand::Cd` — 切换工作目录（cwd 变了进程上下文失效）

`/stop` 不需要 invalidate — 中断成功后进程仍可复用。

---

## Step 3: `src/app/turns.rs` — 签名和错误处理

### 3a. `process_turn` 签名变更

```rust
pub(super) async fn process_turn(
    shared: Arc<AppShared>,
    cancel_slot: Arc<StdMutex<Option<CancellationToken>>>,
    server: &mut Option<SessionAppServer>,  // 新增
    queued: QueuedTurn,
) -> Result<()>
```

### 3b. 调用 `run_turn_with_server` 替代 `run_turn`

```rust
let run_result = shared.codex.run_turn_with_server(
    server, &session, &runtime_request, cancel.clone(), on_event_closure
).await;
```

### 3c. stale thread 错误时清理 server

在 `should_reset_session_after_error` 为 true 的分支中：

```rust
if should_reset_session_after_error(&error) {
    // 现有逻辑：clear_session_conversation
    // 新增：丢弃持久进程
    if let Some(s) = server.take() {
        let _ = s.shutdown().await;
    }
}
```

---

## 进程状态机

```
None ──(首次turn)──► Alive
                       │
         ┌─────────────┼─────────────┐
         │             │             │
    turn 正常完成   turn 中断成功   turn 崩溃/超时
         │             │             │
      保持 Alive    保持 Alive    take() + shutdown → None
         │             │             │
      下次复用      下次复用      下次自动重建
```

**进程被丢弃的场景**：
- 子进程退出（`try_wait` 检测到）→ respawn
- Cancel 超时（5s deadline）→ 进程可能不健康
- `/new`、`/clear`、`/cd` 命令 → 上下文失效
- `should_reset_session_after_error` → stale thread
- Worker 退出（channel 关闭）
- 5 分钟空闲超时

---

## 验证方法

1. `cargo check` + `cargo test` 通过
2. 启动 bot，发送 `/help`，确认日志有 `spawning codex app-server...` + `initialize`
3. 发送第二条消息 `/status`，确认日志**没有**再次 `spawning`，直接 `thread/resume` + `turn/start`
4. 发送 `/new`，确认日志有 `invalidating app-server`
5. 再发消息，确认日志重新 `spawning`
6. 等待 5 分钟不发消息，确认日志有 `idle timeout: shutting down`
7. 再发消息，确认自动 respawn
