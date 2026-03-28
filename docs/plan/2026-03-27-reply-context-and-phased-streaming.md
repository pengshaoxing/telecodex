# 2026-03-27 引用回复上下文 & 流式消息分阶段展示

## 概述

本次改动包含两个独立功能：

1. **引用回复上下文**：支持用户在 Telegram 中通过 reply-to-message 引用消息与 Codex 交互
2. **流式消息分阶段展示**：将 Codex 多阶段输出拆分为独立的 Telegram 消息，保留中间阶段性思考历史

---

## 功能 1：引用回复上下文

### 问题

用户在 Telegram 中引用（reply）某条历史消息后发送新消息时，Codex 不知道用户在引用哪条消息。缺少上下文导致 Codex 需要猜测用户意图。

### 方案

解析 Telegram `reply_to_message` 字段，提取被引用消息的文本，注入到 Codex 的 prompt 中。

### 改动文件

| 文件 | 改动 |
|------|------|
| `src/telegram.rs` | `Message` 结构体新增 `reply_to_message: Option<Box<Message>>` |
| `src/models.rs` | 新增 `ReplyContext { text, is_from_bot }` 结构体；`TurnRequest` 新增 `reply_context` 字段 |
| `src/app.rs` | 新增 `ReplyResolution` 枚举（`Context` / `ToolMessage` / `Ignored`）；`resolve_reply()` 函数；工具消息引用时发送提示通知 |
| `src/app/turns.rs` | `prepare_runtime_request()` 中注入引用上下文到 prompt |

### 设计要点

- **工具消息检测**：通过 emoji 前缀（`🔧📝🔌🔍🤖⚡⏳💭⚠️`）识别工具调用消息，避免无意义的引用
- **工具消息引用处理**：
  - 普通消息引用工具消息 → 发送 `⚠️ Tool action messages cannot be referenced` 提示并终止
  - 命令中引用工具消息 → 静默忽略
- **通知消息自引用防护**：`⚠️` 前缀加入跳过列表，防止用户引用通知消息导致循环
- **上下文注入格式**：
  - 引用 bot 消息：`[The user is replying to your message: "..."]`
  - 引用他人消息：`[The user is referring to: "..."]`
  - 文本截断至 500 字符

### 相关 commits

- `d149a38` Add reply-to-message context support
- `59ede58` Notify user when replying to a tool action message
- `c92a527` Add ⚠️ to skip prefixes so notification messages cannot be referenced

---

## 功能 2：流式消息分阶段展示

### 问题

Codex 的多阶段输出（文本 → 工具调用 → 更多文本 → 最终总结）全部在同一个 Telegram 消息中增量刷新。最终只留下最后的总结文本，用户无法回顾中间阶段性思考和分析过程。

**根因**：

1. Codex 协议中的 `item/completed { type: "agentMessage" }` 是明确的阶段边界信号，但被映射为与流式 delta 相同的 `CodexEvent::AssistantText`，消费端无法区分
2. `summary.assistant_text` 累积器在多个 agentMessage 之间不重置，后续阶段的 `AssistantText` 事件携带所有阶段的拼接文本

### 方案

暴露 Codex 协议中的阶段边界信号，在每个阶段完成时将当前流式消息固定为永久消息，然后用新消息继续展示下一阶段。

### 改动文件

| 文件 | 改动 |
|------|------|
| `src/codex.rs` | `CodexEvent` 新增 `AgentMessageCompleted` 变体；`RunSummary` 新增 `phase_text` 阶段累积器；delta 处理改用 `phase_text`；`item/completed` 发送阶段完成信号后重置 |
| `src/app/turns.rs` | `handle_event` 新增 `AgentMessageCompleted` 处理；新增 `finalize_phase()` 方法；`finish()` 清理空尾部占位符 |

### 设计要点

- **阶段累积器分离**：`phase_text` 只跟踪当前阶段文本（用于 `AssistantText` 事件），`assistant_text` 累积所有阶段文本（用于持久化），以 `\n\n` 分隔
- **`finalize_phase()` 方法**（类似 `flush_tool_actions` 的 rotate 模式）：
  1. 强制写入当前文本到最后一条消息
  2. 发送新占位符消息
  3. `messages.clear()` 脱离已固定消息（不再编辑）
  4. 重置流式状态（`pending_text`、`has_assistant_text`、`last_flushed_text`）
- **空占位符清理**：`finish()` 检测最后一条消息无实质内容时直接删除，避免留下空消息
- **单阶段兼容**：单阶段 turn 的最终效果与之前一致（多一次创建+删除占位符的 API 调用）

### Telegram 消息展示效果

```
[Phase 1: 我来查看一下最近变更...]    ← 固定消息
[🔧 Running `git log`]                ← 工具调用记录
[Phase 2: 根据变更记录分析...]        ← 固定消息
[🔍 Searching the web...]             ← 工具调用记录
[Final: 综合分析结果如下...]           ← 最终输出
```

### 事件流转详解

```
Codex 协议事件                          CodexEvent                    LiveTurnSink 行为
─────────────                          ──────────                    ──────────────
item/agentMessage/delta                AssistantText(phase_text)     编辑流式消息
item/completed {agentMessage}          AssistantText(final) +        finalize_phase(): 固定消息 → 新占位符
                                       AgentMessageCompleted
item/started {commandExecution}        ToolAction("🔧 Running...")  缓冲 → 400ms rotate
item/completed {commandExecution}      Progress("completed")        编辑占位符
item/agentMessage/delta                AssistantText(new_phase)     编辑新占位符
item/completed {agentMessage}          AssistantText(final) +        finalize_phase()
                                       AgentMessageCompleted
turn/completed                         —                            finish(): 删除空占位符
```

### 相关 commits

- `49a3738` Preserve intermediate agent messages as separate Telegram messages
