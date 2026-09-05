# Grok / Antigravity 画布侧栏接入

## 当前结果 · 2026-09-05

源码接入与专项验证已推进到真实 CLI 阶段；主整合已统一安装正式 App，安装读回及剩余原生验收见 [整合报告](canvas-rust-repair.md)。下文真实 CLI 专项证据不代替正式 UI 的完整工具验收。产品范围以 `docs/overview/product-requirements.md` 为准，主要对应 R1、R5、R6、R7。

| 验收 | Grok | Antigravity |
| --- | --- | --- |
| 官方本机登录与协议 | ACP / cached_token，1.0.13 | stream-json，1.1.26 |
| 真实只读用户提示 | 已发送一次，成功 | 已发送一次，修复前失败 |
| 真实画布工具调用 | get_canvas_summary 一次 | 零次；没有伪装成功 |
| 最终答复 | 正确标题及 48 节点 | 明确工具不可用 |
| 后续重试 | 没有 | 没有再发送模型消息 |
| 图片输入 | 本机 image=false，明确拒绝 | 当前流输入仅 text，明确拒绝 |

真实提示使用绑定目录 `.infinite-canvas/project.json` 及正式 SQLite 只读查询得到的快照：`DUkqxVcwRh30uwMAskyxt`，标题「案例4-针脚与矢车菊：克兰奇杀妻案 (5集全本)」，48 节点。测试 MCP 只返回该快照，不写正式画布，不生成媒体。它证明真实 CLI→Rust MCP 的调用，不冒充正式 UI 写入验收。

## 授权与证据

已按用户授权备份 Antigravity settings，仅追加 `permissions.allow` 中的 `mcp(xiaochens_canvas_sidepanel/*)`；整个 JSON 读回与预期对象相同，其他字段保持。没有更换登录或 API Key 计费，没有全局关闭插件。

证据目录：`../infinite-canvas-backups/agent-integration/20260905-101346/`（相对仓库根目录）。备份及证据文件仅本机用户可读：

- `antigravity-settings-before.json`：原配置备份；不在报告中复制敏感内容。
- `readonly-summary.json`：上述实时快照。
- `grok-readonly-result.json` / `antigravity-readonly-result.json`：各家一次真实提示结果。
- `*-prompt-reserved.json`：create_new 单次发送账本，禁止自动重跑。

配置修改前 SHA-256：`c63db57d078f0a6adea4330bdfac06d421157fdd6a4a92f3a083ff1148e9ce75`；修改后：`bbd0ef21f9c0543a6153331c9c6a39061dd47dc6328fa4ec4b6dd21a47f6de3c`。

Grok 原生工具记录：search_tool 发现工具，use_tool 调用 `xiaochens_canvas_sidepanel__get_canvas_summary`，均完成。一次用户提示内部包含 3 次模型调用；原生累计 usage 为 input 34842、output 930、total 35772，不作为当前上下文百分比或费用金额。实际模型 Grok 4.6。

Antigravity 原测试模型为 gemini-3.8-flash-high，工具零次。CLI 日志明确报自定义 Agent not found、falling back to default；未把其自然语言回复当作工具成功。

## Antigravity 根因与修复

1. CLI 1.1.26 在选择 `--agent` 时未发现项目目录定义。多个零模型探针证实，官方全局 `~/.gemini/config/agents/` 可被识别。因此生产代码仅在新对话创建唯一命名、权限 0600 的临时定义，关闭/断线清理自己的文件，不覆盖已有 agent。强制杀死 App 可能留下失效定义，不能保证退出清理。
2. 正确的 MCP 字段为顶层 serverUrl。原生保存的 agent 配置已核对：包含精确 MCP 地址、服务器名、call_mcp_tool 和 finish；没有 run_command/generate_image。init 列出的默认工具清单不能作为实际自定义 agent 生效证据。
3. 恢复对话时 CLI 忽略 `--agent`，沿用原快照。已给每个来源/画布/聊天保存私有 MCP 端口及随机令牌，恢复时绑定同一地址。缺失或占用时明确失败，不悄悄换地址。恢复仍校验原生会话 ID 归属。
4. 每次启动写独立私有诊断文件；发现 fallback 则阻止就绪。前端等待 init，后端拒绝 init 前发送 user，防止错误助手收到模型消息。诊断文件退出清理。

这些修复后的真实模型工具调用仍未再验证；保留修复前失败记录，不超出各家一次的发送范围。修复后零模型真实 CLI 初始化和恢复均通过：生产 command 工厂、同一原生会话 ID、精确 MCP 地址、限定工具快照均一致，两轮 gen_metadata 均为 0。

## 实现与权限边界

后端全部位于 `desktop/src-tauri/src/canvas_local_agent.rs`，使用现有 Axum/Rust 依赖和 MCP 消息结构，没有新增 Node/Go 后端。CLI 自有进程组在关闭/异常时回收。Grok 使用 Markdown frontmatter profile、ACP 缓存登录；不提供客户端文件/终端能力。Grok 仍会发现本机插件，不能宣称完全隔离，工具权限交给侧栏单次确认。

前端 `canvas-local-agent-runtime.ts` 使用原画布 action 执行器，文字按 100ms 合并，工具串行。媒体/节点删除/连线删除沿用用户确认；Grok 权限与媒体确认共用面板队列，取消或断线使旧确认失效。重复 MCP 请求 ID 同参数重放结果；执行中或参数冲突拒绝重复，不重复提交任务。loopback 仅监听 127.0.0.1 并拒绝 Origin，地址含随机能力令牌。

图片附件在发送前明确拒绝，不静默丢图；节点文字仍可引用。Codex/API 媒体解析由整合任务统一修复，面板已交接。新增 onModel 回调仅取 Grok 官方 session.models.currentModelId 与 update._meta.modelId；实测为 grok-4.6。Antigravity init/result 未提供模型字段，界面保留未知，不能将私有记录里曾出现的模型当作当前连接名称。模型与用量未知时不编造窗口百分比。官方工具可能保存原生对话，画布与原生客户端不是完全自动同步的两个界面。

## 自动验证

- 前端真实 runtime 协议替身：新增 12 项 + Codex 14 项，共 26 项通过。覆盖初始化失败零发送、恢复、流式与最终答复、真实 action 归一化、布局限制、图片拒绝、取消和确认队列。
- Rust 全量 51 项通过、3 个 opt-in 跳过；其中专项 7 项通过，覆盖会话归属、输入限制、配置清理、进程回收、MCP 往返/Origin/去重、固定地址恢复/占用拒绝、fallback 阻止。
- 真实模型测试与零模型 CLI 测试默认 ignore，不能执行笼统 `--ignored`，否则会触发额外真实测试。真实模型测试另有持久化单次账本。
- 早期 Next 生产构建通过；当时原有 tsc 八处问题已交整合任务。当前全量类型、构建、正式 UI 与数据核验以整合报告为准，不用旧构建冒充本次最终结果。

可安全复现（无模型）：

```sh
node --test docs/progress/canvas-local-agent.test.mjs docs/progress/canvas-codex.test.mjs
CARGO_TARGET_DIR=/tmp/canvas-agent-specialist-target cargo test --manifest-path desktop/src-tauri/Cargo.toml --lib canvas_local_agent
```

零模型真 CLI 专项需精确选择 `antigravity_real_startup_resume_without_model`，只发送控制初始化事件，核对原生会话 gen_metadata 行数为 0 及保存的 agent 配置；不输出原生思考内容。

## 官方参考

- [Grok Headless & Scripting](https://docs.x.ai/build/cli/headless-scripting)：ACP、缓存认证和请求/通知结构。
- [Antigravity Headless mode](https://antigravity.google/docs/cli/headless/)：stream-json、会话恢复、只支持 text、控制消息不支持。
- [Antigravity Subagents](https://antigravity.google/docs/subagents)：项目 agent 发现目录、tools、mcpServers。
- [Antigravity Permissions](https://antigravity.google/docs/cli/permissions/)：MCP 默认 Ask 及服务器级允许规则。
- [Antigravity Changelog](https://antigravity.google/changelog)：inheritCustomizations 配置的官方说明。说明不代替本机生效测试。
