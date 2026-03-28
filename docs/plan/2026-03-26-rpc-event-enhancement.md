# 2026-03-26 Codex App-Server RPC 事件增强分析

## 1. 现状

Telecodex 当前仅处理 6 个 RPC 通知事件：

| 事件 | 用途 |
|---|---|
| `thread/started` | 记录 thread_id |
| `turn/started` | 记录 turn_id |
| `turn/completed` | 结束 turn，检查 fail/interrupted |
| `item/agentMessage/delta` | 流式文本增量 → 编辑 Telegram 消息 |
| `item/started` (仅 commandExecution) | 显示 `Running \`command\`` |
| `item/completed` (agentMessage + commandExecution) | 命令完成状态 |

协议实际暴露了 **40+ 种通知事件**，大量被静默丢弃（`_ => {}` catch-all）。

## 2. 协议中可利用的事件（按价值分级）

### 高价值 — 直接增强交互体验

| 事件 | 参数 | 可实现效果 | 复杂度 |
|---|---|---|---|
| `item/reasoning/summaryTextDelta` | `delta: string, summaryIndex, itemId` | 实时展示 thinking 摘要（"正在思考：分析代码结构…"） | 低 |
| `item/reasoning/textDelta` | `delta: string, contentIndex, itemId` | 完整 reasoning 文本增量（比 summary 更详细） | 低 |
| `item/commandExecution/outputDelta` | `delta: string, itemId` | 命令执行中实时输出（不用等 completed） | 中 |
| `item/fileChange/outputDelta` | `delta: string, itemId` | 文件修改中的 diff 增量 | 中 |
| `item/mcpToolCall/progress` | `message: string, itemId` | MCP 工具调用进度 | 低 |

### 中价值 — 状态感知

| 事件 | 参数 | 可实现效果 | 复杂度 |
|---|---|---|---|
| `thread/tokenUsage/updated` | `tokenUsage: {last, total}` | 每 turn 后展示 token 消耗 | 低 |
| `turn/diff/updated` | `diff: string` | turn 级别聚合 diff，可发送代码变更摘要 | 低 |
| `model/rerouted` | `fromModel, toModel, reason` | 模型被降级时通知用户 | 极低 |
| `account/rateLimits/updated` | `rateLimits: RateLimitSnapshot` | 实时更新限流状态 | 低 |

### 低价值 — 锦上添花

| 事件 | 说明 |
|---|---|
| `item/started` (reasoning/fileChange/mcpToolCall/webSearch) | 扩展当前只处理 commandExecution 的逻辑 |
| `thread/name/updated` | 自动同步 session 标题 |
| `thread/compacted` | 上下文压缩时提示用户 |
| `item/reasoning/summaryPartAdded` | reasoning 摘要分段完成 |

## 3. 推荐实施顺序

### Phase 1（最小增强，本次实施）

- **`item/reasoning/summaryTextDelta`**：thinking 摘要实时展示
- **`item/started` 扩展所有 item 类型**：reasoning / fileChange / mcpToolCall / webSearch 开始时显示对应标签

### Phase 2（命令输出实时化）

- **`item/commandExecution/outputDelta`**：长命令实时输出（需截断策略）
- **`item/fileChange/outputDelta`**：文件修改 diff 实时展示

### Phase 3（成本与状态监控）

- **`thread/tokenUsage/updated`**：token 消耗展示
- **`turn/diff/updated`**：turn 结束时发送变更摘要
- **`model/rerouted`**：模型降级通知
- **`account/rateLimits/updated`**：限流状态自动更新

## 4. item/started 中可识别的 ThreadItem 类型

（来自 `codex app-server generate-json-schema`）

| type enum | 说明 |
|---|---|
| `userMessage` | 用户消息 |
| `agentMessage` | Agent 回复文本 |
| `plan` | (EXPERIMENTAL) 计划输出 |
| `reasoning` | 思考/推理过程 |
| `commandExecution` | 命令执行（**已处理**） |
| `fileChange` | 文件修改 |
| `mcpToolCall` | MCP 工具调用 |
| `dynamicToolCall` | 动态工具调用 |
| `collabAgentToolCall` | 协作 Agent 工具调用 |
| `webSearch` | 网络搜索 |
| `imageView` | 图片查看 |
| `imageGeneration` | 图片生成 |
| `enteredReviewMode` | 进入 review 模式 |
| `exitedReviewMode` | 退出 review 模式 |
| `contextCompaction` | 上下文压缩 |

## 5. 关键 Schema 结构参考

### ReasoningSummaryTextDeltaNotification
```json
{
  "method": "item/reasoning/summaryTextDelta",
  "params": {
    "delta": "string",      // 增量文本
    "itemId": "string",
    "summaryIndex": "int64", // 摘要段落索引
    "threadId": "string",
    "turnId": "string"
  }
}
```

### McpToolCallProgressNotification
```json
{
  "method": "item/mcpToolCall/progress",
  "params": {
    "message": "string",     // 进度消息
    "itemId": "string",
    "threadId": "string",
    "turnId": "string"
  }
}
```

### ThreadTokenUsageUpdatedNotification
```json
{
  "method": "thread/tokenUsage/updated",
  "params": {
    "threadId": "string",
    "turnId": "string",
    "tokenUsage": {
      "last": "TokenUsageBreakdown",
      "total": "TokenUsageBreakdown",
      "modelContextWindow": "int64|null"
    }
  }
}
```
