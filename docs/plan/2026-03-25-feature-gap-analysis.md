# 2026-03-25 Telecodex 功能差距分析

## 当前已支持功能

### Codex 协议（app-server JSON-RPC）
- `turn/start` / `turn/completed` / `turn/interrupt` — 完整
- `item/agentMessage/delta` — 流式文本
- `item/started` / `item/completed` (commandExecution, agentMessage) — 命令执行状态
- `item/commandExecution/requestApproval` — 命令审批（InlineKeyboard）
- `item/fileChange/requestApproval` — 文件变更审批（InlineKeyboard）
- `item/tool/requestUserInput` — 用户输入请求（选择型 + 自由文本）
- `thread/start` / `thread/resume` — 会话管理

### Telegram 消息类型
- 文本消息（Markdown → HTML 渲染，流式编辑）
- 图片/文档/音频/视频 — 附件下载 + 产物上传
- InlineKeyboard 按钮 — 审批、用户输入、会话切换
- Forum Topic — 多工作区管理
- 语音消息 — 本地 Parakeet 转录（可选 feature）

### 命令
- Bridge 命令：`/new`, `/login`, `/logout`, `/model`, `/sandbox`, `/cd`, `/pwd`, `/prompt`, `/sessions`, `/environments`, `/stop`, `/review`, `/clear`, `/role`, `/allow`, `/deny`, `/restart_bot`, `/think`, `/topic`, `/use`
- Forward 命令（转发给 Codex）：`/help`, `/status` 等

---

## 待完善功能分析

### P0 — 高优先级（影响核心使用体验）

#### 1. 自定义 Whisper API 端点支持
**现状**：语音转录仅支持本地 Parakeet 模型（需 ONNX），无远程 API 选项。
**需求**：支持 OpenAI-compatible `/v1/audio/transcriptions` 端点（硅基流动 SenseVoice、Groq Whisper 等），配置 `base_url`、`api_key`、`initial_prompt`。
**预估改动**：~120 行，涉及 `src/config.rs`、`src/transcribe.rs`、`src/app/turns.rs`

#### 2. Markdown 渲染不完整
**现状**：`src/render.rs` 的 `render_markdown_to_html` 是简易实现，只支持：
- 代码块 (```)
- 粗体 (**)、斜体 (*/_)、行内代码 (`)
- 链接 [text](url)

**缺失**：
- 标题（# ## ###）→ Telegram 支持 `<b>` 模拟
- 有序/无序列表 — Codex 经常输出列表
- 引用块（>）→ Telegram 支持 `<blockquote>`
- 删除线 (~~)
- 水平线 (---)
- 嵌套格式（如粗体内的代码）

**影响**：Codex 回复中的列表和标题在 Telegram 中显示为纯文本，格式丢失。

#### 3. 优雅停机缺失
**现状**：`src/app.rs:183` Ctrl+C 直接 return，所有进行中的 turn 被丢弃。codex 子进程没有收到信号，Telegram 上的占位消息永远停在 ⏳ 状态。
**需求**：
- Ctrl+C → 取消所有活跃 turn 的 CancellationToken
- 等待所有 worker 完成（最多 N 秒）
- 更新占位消息为 "Bot stopped" 或保留最终内容
- 然后再退出

### P1 — 中优先级（改善运维和体验）

#### 4. Worker Map 无限增长
**现状**：`src/app.rs:1399` `worker_for()` 插入 workers map 后从不清理。每个曾访问的 session 永久占用一个 entry。长时间运行或 forum 模式多 topic 下内存缓慢增长。
**修复**：worker 的 tokio task 结束时从 map 中移除自己，或定期清理已关闭的 channel。

#### 5. 长消息的代码块截断问题
**现状**：`split_text` 按字符数切割文本，不感知 markdown 代码块边界。如果切割点在 ``` 内部，拆分后的两段各自 HTML 渲染不正确：
- 第一段代码块不闭合
- 第二段没有开头的 ```
**需求**：`split_text` 在代码块内部切割时，自动补全闭合标记和开头标记。

#### 6. Telegram 消息长度的 HTML 维度检查
**现状**：`max_text_chunk` (3500) 限制的是 markdown 源文本字符数，但 Telegram 限制的是 HTML 字符数（4096）。Markdown 渲染后 HTML 可能膨胀（尤其是含链接和 HTML entity 转义的内容），导致仍然超限。
**需求**：在 `flush` 中 render HTML 后检查实际长度，超限则回退到更小的 chunk。

#### 7. 文件上传大小限制
**现状**：`send_generated_artifacts` 直接上传文件，没有检查大小。Telegram Bot API 限制为 50MB（document）和 10MB（photo）。超大文件上传会失败并报 400 错误。
**需求**：上传前检查文件大小，超限时发送提示消息而不是静默失败。

#### 8. 多字段 UserInput 支持
**现状**：`request_telegram_user_input` 只处理第一个 field（MVP 实现）。如果 Codex 请求多个 field，只有第一个被展示。
**需求**：依次展示多个字段，收集所有答案后一次性返回。

### P2 — 低优先级（锦上添花）

#### 9. 会话上下文持久化
**现状**：`codex app-server` 每个 turn 都是新进程，通过 `thread/resume` + `codex_thread_id` 恢复上下文。但如果 Codex 本地的 thread 数据被清理，session 断裂。
**需求**：telecodex 自身也保存对话历史摘要，在 thread 不可用时提供 context fallback。

#### 10. 消息引用/回复支持
**现状**：用户在 Telegram 中 reply 一条旧消息时，被回复的消息内容不会传给 Codex。
**需求**：提取 `reply_to_message` 的文本，作为上下文前缀传给 Codex。

#### 11. 反应/Reaction 支持
**现状**：用户无法通过 emoji reaction 与 bot 交互（如 👍 表示满意）。
**可选**：用 reaction 实现快速反馈，如 👍 = "good job, continue"，👎 = "redo this"。

#### 12. 进度条/百分比展示
**现状**：流式更新时只有不断编辑的文本内容，用户不知道大概还要多久。
**可选**：在 status text 中加入 token 消耗计数或 elapsed time。

#### 13. 多用户并发隔离
**现状**：每个 session (chat_id + thread_id) 有独立 worker，但同一 session 的多个用户共享状态。admin 可以抢其他用户的 approval。
**可选**：per-user turn 队列，避免用户间干扰。

#### 14. Webhook 模式
**现状**：仅支持 long polling。对于高流量场景，webhook 更高效且延迟更低。
**预估**：需要 HTTP server（如 axum）+ TLS 证书配置，改动较大。

---

## 配置缺口

| 缺失配置项 | 说明 |
|-----------|------|
| `log_level` | 允许在 toml 中配置日志级别，而不依赖 `RUST_LOG` 环境变量 |
| `whisper.base_url` / `whisper.api_key` / `whisper.model` | 远程 Whisper API 配置 |
| `shutdown_timeout_seconds` | 优雅停机超时 |
| `max_concurrent_turns` | 全局并发 turn 数限制 |
| `max_upload_size_mb` | 文件上传大小限制 |

---

## 建议优先级排序

1. **P0-3 优雅停机** — 防止运维事故
2. **P0-2 Markdown 渲染** — 直接影响每条消息的展示质量
3. **P1-5 代码块截断** — 长回复常见问题
4. **P0-1 Whisper API** — 解锁语音交互
5. **P1-4 Worker 清理** — 长期运行稳定性
6. 其他按需排序
