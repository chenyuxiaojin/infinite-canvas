# 画布侧栏 Codex 正式安装与只读验收

日期：2026-09-04，约 22:00–22:05（Asia/Singapore）。

## 结果与授权范围

用户回复“做”，授权备份旧版/数据、更新并重启正式画布，以及一条消耗少量 Codex 额度的只读消息。已完成。未生成媒体，未变更原节点/连线、AI 登录或全局配置；未提交、推送、发布或改版本号。

正式入口仍为 `/Users/chenhuajin/Applications/小陈的画布.app`，版本 1.0.0，identifier 与数据目录不变。使用 `desktop` 的 `build:app` 完成 Next、Agent CLI、Go、Rust 及 App 打包，再由正式安装器签名、备份并替换。严格签名验证通过，构建目录的 App 已移走，不额外保留可运行副本。旧安装包在废纸篓可恢复。

“本机 Codex”指本机官方 CLI / App Server 连接，不是离线模型。模型推理仍连接 Codex 云端并使用现有 ChatGPT 登录和相应额度。参考官方接口文档：[App Server](https://learn.chatgpt.com/docs/app-server)。

## 真实只读首测

- 原生 App 打开明确绑定的案例 4：`DUkqxVcwRh30uwMAskyxt`。没有按标题猜测，也没有选首页另一个同名画布。
- 侧栏切换“本机 Codex”新建独立对话，保留原 API 历史；发送 1 条用户消息，要求只调用一次 `get_canvas_summary` 并报告标题与节点数，不得创建/修改/删除/生成。
- UI 显示配置模型 `gpt-5.6-sol`。真实最终答复：“画布标题：案例4-针脚与矢车菊：克兰奇杀妻案 (5集全本)；节点总数：48”。
- 原生 Codex 会话 `01a06cba-92a9-7403-95cd-f48df905fcb7` 的记录有 1 次动态工具执行：`const r = await tools.get_canvas_summary({}); text(r);`。有 1 个 task_started、1 个 task_complete，没有重发消息、没有其他画布工具调用。
- 该消息内部进行了 2 次模型请求（调用工具前/得到工具结果后）。累计输入 46,670、输出 77；最后一次输入 24,276、输出 38，窗口 258,400。UI 的上下文近似比例使用最后一次 24,314 / 258,400，四舍五入显示 9%，不是累计计费或账号余额。
- Codex 曾提示技能描述因技能上下文预算被缩短。该占用包含指令、工具、技能目录等，不能理解为画布本身用了 9%。未精简全局技能或验证真实长会话压缩。
- 测试结束后，App 自有 Codex 子进程已退出；Go/Node 后台服务继续正常运行。保留画布及已完成答复供用户试用。

## 数据读回

使用停止 App 后的 before.db 和真实回复后的 after.db，运行现有 `verify-repair-data.mjs` 对全部 19 张表进行逐行哈希与画布字段比对：

- 两份 SQLite 完整性均为 `ok`，结构一致。
- 18 张非画布表全部相同；10 条画布记录无增删，所有原节点和连线相同。
- 仅案例 4 的 `activeChatId`、`chatSessions`、`updatedAt` 与数据库更新时间变化，符合新增对话；其 48 个节点和 19 条连线不变。
- `project-media` / `agent-media` 共 65 个文件哈希一致；4 个已有片子/验收目录绑定、4 个项目 Codex 配置、全局 Codex 配置共 9 个文件哈希一致。没有向这些配置写入凭据或新设置。
- WebKit/IndexedDB 已完整备份，本轮没有逐项 Blob/LocalStorage 对账，不将 SQLite 结果扩展为浏览器全部存储对账。

## 回归与剩余范围

安装前审查发现：异常断线可能留下旧的媒体/删除确认。已补上运行层同步失效回调、面板 controller 中止和确认取消；关闭回调重入由 settled/closed 守卫处理。新增回归覆盖“断线后晚到 true 不得写入”，独立只读复核通过。

前端 Codex 14 项与本地工作流 11 项，共 25 项通过。此前全量 Rust 43 项通过，1 项原图实机 opt-in 跳过。安装前再次全量 TypeScript 检查仍为既有四文件八处诊断，本次改动文件无新增；Next 生产构建依照项目配置跳过全量类型检查。不能把生产构建成功称为全量类型检查通过。

尚未实测：第二轮会话恢复、选中图片引用、真实模型运行时取消、上下文压缩及 70%/85% 提醒触发、生成/删除确认、长时间稳定性。只读沙箱约束的是 Codex 自身环境，不是所有画布工具的只读锁；本轮安全依据是明确只读消息、实际工具记录及原节点/连线前后对账。

## 备份与证据位置

- 数据备份和前后数据库：`/Users/chenhuajin/项目/自己的应用/infinite-canvas-backups/local-installs/codex-integration-20260904-220056/`，包含 `application-support-before.zip`、`webkit-before.zip`、`before.db`、`after.db`、`comparison.json`、`readonly-acceptance.json`。两个 ZIP 完整性检查通过；目录仅本用户可访问。
- 旧 App ZIP：`/Users/chenhuajin/项目/自己的应用/infinite-canvas-backups/local-installs/小陈的画布-2026-09-04T14-01-56-175Z-11389.zip`，校验通过。
- 旧安装包：`/Users/chenhuajin/.Trash/小陈的画布-替换前-2026-09-04T14-01-56-175Z-11389.app`。未永久删除或清空废纸篓。
- 原生 Codex 日志：`/Users/chenhuajin/.codex/sessions/2026/09/04/rollout-2026-09-04T22-02-41-01a06cba-92a9-7403-95cd-f48df905fcb7.jsonl`。报告只摘取验收所需计数/工具名，不复制配置、账号信息或画布正文。
