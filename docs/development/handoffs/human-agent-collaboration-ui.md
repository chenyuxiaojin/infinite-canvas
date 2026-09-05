# 无限画布人与 Agent 共编 UI 交接

## 交付边界

- 分支：`feat/human-agent-collaboration-ui`
- 基线：`e00acb71cf02dcc04e78f13ec76b63b6413a93d0`（`feat/macos-director-console`）
- 交付提交：以本文件所在分支 HEAD 为准。
- 只修改 React/Next 画布、项目数据结构、测试和文档；没有新增 CLI、Agent HTTP 服务、Tauri IPC 或 Rust 执行器。
- 没有调用付费模型，没有读写 Eagle、达芬奇或正式用户媒体。

## 已实现

1. 画布顶部以图标和文字显示 Agent 待命、执行中、完成、失败或冲突，以及当前 revision；状态变化通过 `aria-live` 提示。
2. Agent 新建或修改节点在 9 秒内显示带文字的 `Agent 刚修改` 状态和虚线轮廓，不只依赖颜色。
3. 每个节点可人工锁定/解锁。锁只阻止 Agent 覆盖，人工拖拽、标题/正文、提示词、模型、参考素材、时长、尺寸等原有编辑路径仍可用。
4. Agent 批次历史保存操作者、时间、摘要、动作、影响节点、结果和 revision。最近一个可逆批次在没有后续修改时可撤销；后续人工 revision 会明确禁用撤销。
5. `nodes`、`connections` 与 `collaboration` 保存在同一个 `CanvasProject`，同步写入 Zustand 持久化、远端项目 JSON和 v4 ZIP 导入导出，不维护第二套画布状态。
6. Agent 每个写动作先检查批次预期 revision 和目标节点锁；执行期间发生人工提交后，后续写动作停止并显示冲突，不能静默覆盖。

## 统一操作核心总装接线

统一操作核心尚未出现在本基线，因此本分支只建立薄 adapter，没有复制画布 reducer：

- 接口与临时实现：`web/src/app/(user)/canvas/agent/canvas-collaboration-adapter.ts` 的 `CanvasCollaborationAdapter`。
- Agent 生命周期入口：`CanvasAssistantPanel` 的 `onAgentRunStart`、`onAgentRunProgress`、`onAgentActionResult`、`onAgentRunComplete`。
- 画布接线点：`canvas-client-page.tsx` 的 `startCanvasAgentBatch`、`noteCanvasAgentActionResult`、`finishCanvasAgentBatch`、`executeCanvasAgentAction`。
- 现有 `executeCanvasAgentAction` 仍调用原画布操作实现，adapter 只在前后读取同一份 `nodes/connections`、做 guard、记录 revision/undo 和添加节点标记。

总装时让统一核心返回标准 mutation/result，然后在上述四个生命周期点调用它；用核心的受控操作替换 `executeCanvasAgentAction` 内部动作执行即可。保留 `CanvasCollaborationAdapter` 的 guard/history 接口或给它做实现替换，不要再复制一套 reducer，也不要新建独立的 Agent canvas store。统一核心必须接收当前 `CanvasProject` revision、节点 `collaboration.locked` 和最新 `nodes/connections`；操作成功后仍写回现有 React state，再由 adapter 记录批次。

当前有意保留的限制：`delete_node` 和媒体生成批次标记为不可逆；可逆撤销要求当前 revision 恰好等于批次结束 revision。这是保护人工结果的 fail-closed 约束，不应在总装时放宽为静默覆盖。

## 自动化与零付费验收

```sh
cd /Users/chenhuajin/项目/自己的应用/infinite-canvas-worktrees/human-agent-collaboration-ui/web
bun run test:collaboration
bun run build
bun x tsc --noEmit
```

`test:collaboration` 的 9 个测试覆盖锁、无写入冲突批次历史、revision 冲突、字段级撤销、后续人工修改阻止撤销、不可逆媒体/删除，以及状态组件的可访问文字。`tsc` 在本基线仍报告与本功能无关的既有错误，Next 生产构建通过；总装时不要把这些基线错误归到 adapter。

无需模型的交互路径：

1. 启动 `bun run start -- -H 127.0.0.1 -p 3187`，新建或打开画布。
2. 在 URL 后加 `?agent-collab-demo=1`，点击顶部 Agent 状态，再点“运行零付费本地协作演示”。
3. 确认新建可编辑文本节点、`Agent 刚修改`、revision 增加、历史中显示操作者/摘要/节点。
4. 锁定节点，确认节点显示“已锁”；旧批次撤销因人工 revision 变化而禁用并给出保护提示。
5. 再运行一次 demo，不做人工修改，立即撤销最近批次；确认第二个节点消失、第一节点和锁保留、history 标为“已撤销”。

现场在 1440×900 浏览器完成以上路径：第一次 Agent 批次 rev 1，人工锁后 rev 2；第二次批次 rev 3，撤销后 rev 4，节点数从 2 回到 1。浏览器仅因未启动 Go API 出现既有 `/api/settings` 502，不影响本地画布协作链路，未出现本功能新增 console error。
