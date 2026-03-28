# 2026-03-26 Telecodex 模式切换支持分析（plan/default）

## 1. 结论摘要

- Telecodex 当前没有 `plan/default` 协作模式的业务层入口，`/plan` 会被当成普通文本转发。
- 现有架构已经具备“按 turn 传参覆盖并在同一 session 持续生效”的基础能力（`turn/start` 每次都显式带策略参数），不需要重启进程即可切换。
- 要支持 `plan/default`，关键是三件事：
  1. 会话层持久化一个 `collaboration_mode` 字段；
  2. 命令层新增 `/mode`（及 `/plan`、`/default` alias）；
  3. `turn/start` 构造 `collaborationMode`（而不是固定 `null`）。

## 2. 当前实现现状（代码事实）

### 2.1 命令层

- `BridgeCommand` 中没有模式切换命令，解析表也没有 `/plan` 或 `/mode`。见 `src/commands.rs:18-45`、`src/commands.rs:84-151`。
- 未识别命令会走 `ParsedInput::Forward`，即把原文本直接发给 Codex。见 `src/commands.rs:151-154`。
- README 的 Bridge 命令列表也没有 `/plan`。见 `README.md:128-156`。

### 2.2 会话持久化层

- `SessionRecord` 仅保存 `model/reasoning_effort/session_prompt/sandbox_mode/approval_policy/search_mode`，没有 `collaboration_mode`。见 `src/models.rs:57-73`。
- SQLite `sessions` 表也没有该字段。见 `src/store.rs:460-481`。

### 2.3 App-Server 调用层

- `turn/start` 请求中 `collaborationMode` 被硬编码为 `null`。见 `src/codex.rs:813-825`。
- 初始化握手里 `experimentalApi` 固定为 `false`。见 `src/codex.rs:1274-1278`。
- 启动 app-server 时未加 `--experimental`。见 `src/codex.rs:792-797`。

### 2.4 事件消费层

- `CodexEvent` 无计划流专用事件（只有 `Progress/AssistantText/...`）。见 `src/codex.rs:44-50`。
- `handle_notification` 也未处理 `turn/plan/updated` / `item/plan/delta`（默认落入 `_ => {}`）。见 `src/codex.rs:520-617`。

## 3. 协议能力验证（本地 schema）

> 以下来自本机 `codex app-server generate-json-schema` 结果。

- 非 experimental schema 的请求方法里没有 `collaborationMode/list`（仅有 `turn/start` 等）。
- `--experimental` schema 中出现 `collaborationMode/list`。
- `TurnStartParams` 在 experimental schema 下存在 `collaborationMode` 字段，描述为“Set a pre-set collaboration mode... for this turn and subsequent turns”。
- `ModeKind` 枚举包含 `plan`、`default`。
- `ServerNotification` 在当前 schema 下包含 `turn/plan/updated` 与 `item/plan/delta`。

这说明：

- 协作模式切换能力在协议层可用；
- 该能力以“下一次 `turn/start` 参数”为主，不是靠文本命令本身生效；
- 对 telecodex 而言，切换策略是“会话状态 + turn 参数构造”问题。

## 4. 目标行为定义

### 4.1 用户可见行为

- `/mode plan`：设置当前 Telegram session 的协作模式为 `plan`（仅改会话设置，不自动发起推理）。
- `/mode default`：恢复默认模式。
- `/plan`、`/default`：分别等价于 `/mode plan`、`/mode default`（alias）。
- `/mode`（无参数）：显示当前会话模式。

### 4.2 生效时机

- 模式切换从“下一次 `turn/start`”开始生效；
- 正在运行的 turn 不做中途热切；
- 不需要重启 `codex app-server` 进程，也不需要 `thread/resume` 重入才能切换。

### 4.3 与沙箱/审批的关系

- `collaborationMode` 与 `sandboxPolicy`、`approvalPolicy` 是正交维度；
- 同一 turn 可以同时指定 `collaborationMode=plan` 和 `sandboxPolicy=workspaceWrite/dangerFullAccess`。

## 5. 推荐改造方案

### 5.1 数据模型与存储

1. 新增模式枚举（建议字符串存储）：
   - `default`
   - `plan`
2. `SessionRecord` 新增 `collaboration_mode: String`。
3. `sessions` 表新增 `collaboration_mode TEXT NOT NULL DEFAULT 'default'`。
4. `SessionDefaults` 增加默认值（建议固定 `default`；后续可再开放配置项）。

### 5.2 命令模型

1. `BridgeCommand` 增加：
   - `Mode { mode: Option<String> }`
2. 命令解析增加：
   - `/mode [default|plan]`
   - `/plan` -> `Mode { mode: Some("plan") }`
   - `/default` -> `Mode { mode: Some("default") }`
3. `command_help` 增加 `/mode` 快捷提示。
4. README / setup 文档补充命令说明。

### 5.3 运行时参数构造

1. `build_turn_start_params` 中改为按 session 写入 `collaborationMode`：
   - `default` -> `null`（保持向后兼容）
   - `plan` -> 组装 `{"mode":"plan","settings":...}`
2. `settings.model` 是协议必填，建议解析优先级：
   - `session.model`
   - `config.codex.default_model`
   - 若仍缺失：拒绝切 plan 并提示先设置 `/model`

### 5.4 experimental 兼容策略

1. 在 `CodexConfig` 增加开关（例如 `enable_experimental_api`，默认 `false`）。
2. 开关开启时：
   - app-server 启动参数追加 `--experimental`；
   - `initialize.capabilities.experimentalApi = true`。
3. 若用户请求 `plan` 但 experimental 关闭，直接返回明确提示，不 silent fallback。

### 5.5 计划事件展示（可选但推荐）

1. 在 `CodexEvent` 增加计划事件（如 `PlanDelta(String)` / `PlanUpdated(String)`）。
2. 在 `handle_notification` 解析：
   - `item/plan/delta`
   - `turn/plan/updated`
3. `LiveTurnSink` 将计划流映射为进度文本（在 assistant 最终文本前展示）。

## 6. 分阶段实施建议

### Phase A（最小可用）

- 数据层新增 `collaboration_mode`。
- 命令层支持 `/mode`、`/plan`、`/default`。
- `turn/start` 仅在 `plan` 时填充 `collaborationMode`。
- 不做计划增量事件渲染。

### Phase B（可观测性增强）

- 开启并探测 experimental 能力。
- `/mode` 无参数时返回“当前模式 + experimental 状态”。
- 补充失败原因（无模型、experimental 未开、服务端拒绝）。

### Phase C（体验增强）

- 渲染 `turn/plan/updated` / `item/plan/delta`。
- 可选支持 `/plan <prompt>` 原子语义（先切模式再同 turn 发起请求）。

## 7. 测试清单

### 7.1 单元测试

- `parse_command`：
  - `/mode`、`/mode plan`、`/mode default`
  - `/plan`、`/default`
  - 非法参数报错
- `build_turn_start_params`：
  - default 时 `collaborationMode == null`
  - plan 时 `collaborationMode.mode == "plan"`
  - plan 且无可用 model 时触发可预期错误
- `store`：
  - 新列迁移可在旧库上自动补齐
  - 新建 session 默认 `collaboration_mode=default`

### 7.2 集成测试（手工）

1. `/mode plan` 后发普通消息，确认该 turn 在 plan 模式下运行。
2. 不重启进程，`/mode default` 后再次发消息，确认恢复默认。
3. 切换 `sandbox` 与 `mode` 的组合测试（`workspace-write`、`danger-full-access`）。
4. 正在运行 turn 中切模式，确认当前 turn 不变、下一 turn 生效。

## 8. 已知风险与注意事项

- `collaborationMode.settings.model` 必填：必须在 Telecodex 端做前置校验，避免服务端报错。
- `review` 路径使用 `codex exec review`，不是 app-server turn 流；该路径不应强行套用协作模式语义。
- 如果仅做命令层解析、不做 turn 参数透传，用户会被“文本上看似切换成功”误导，这类假阳性需要避免。

