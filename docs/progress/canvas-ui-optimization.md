# 前端与 Agent 体验专项施工

四项源码收尾完成，主整合已构建并安装正式 App；原生交互和内存验收仍待完成，统一见 [整合验收](canvas-rust-repair.md)。本专项没有调用付费模型或删除原素材。

## 当前实现

- 拖动按节点/连线快照构建索引，每帧只处理相邻连线，多选去重；撤销、重做与增删换快照重建索引。拖动节点跨可见边界保持挂载。既有 32ms 平移节流保留。
- 长对话每次只挂载最多 12 条消息，前后页重叠 2 条；历史锚点按消息 ID 固定，新消息不挤走正在阅读的页。仅在底部跟随；选字暂停跟随。完整持久历史不截断，提供全文搜索、命中跳转和全文复制。已完成 Markdown 记忆化。浏览器页面查找仅覆盖挂载页，完整查找使用面板搜索。
- 原图、视频、音频由显示/页面作用域租用，最后消费者释放后撤销临时 URL。全局素材库启动不再全库加载原 Blob，卡片接近视口加载，详情/播放器独立持有。画布和图像/视频工作台、创作工作流离开时释放页面租用；恢复历史通过 restoredRevisions 重挂载清除旧租用、局部撤销状态和定时器。两路读取限制、迟到加载丢弃及重新显示不返回已撤销地址均有隔离验证。
- 上传/导入/生成结果允许只保存稳定键而不留下无消费者 URL；画布和播放器接受仅 storageKey 的成功结果。下载/导出/Agent 文件转换读取原 Blob。元素参考保留原有供应商 URL 协议，本轮不引入新传输格式。
- 桌面端停止依据“当前引用”自动删除原 Blob，保护版本历史。原素材与稳定键不删除、不压缩。完整消费者与范围限制见 [canvas-url-consumers.md](canvas-url-consumers.md)。
- API 上下文最多 120 个目录项，最多 16 个相关节点带正文；连线只扩一跳，不依赖边排列。Codex/local 保留既有精简输入与 Codex 100ms 合并输出。
- 助手消息可展开实际传输与后续工具返回清单：节点目录/正文/附图性质、实际选入的内置 SOP 源模块和 SHA-256。无额外外部文件读取；未确认传输不记为已发送，Antigravity 本机通道与模型接收分开表述。
- 保留真实 Codex 用量与 70%/85% 提醒；其他来源不猜窗口。API 无法确认图像能力明确拒绝，不能静默丢附图。加载/失败或无来源素材不能新设正式参考或定稿。版本历史按钮接在保存状态旁。

## 验证

`node --test docs/progress/canvas-ui-optimization.test.mjs docs/progress/canvas-codex.test.mjs docs/progress/canvas-local-agent.test.mjs docs/progress/canvas-local-image.test.mjs docs/progress/local-workflow.test.mjs`：84 项通过。使用真实前端模块与协议/IPC/URL/React 生命周期替身，零模型调用、零用户数据写入。

索引合成用例为 10,000 节点、9,999 连线、100 次更新，每次返回 2 条相邻连线。历史用例覆盖 1,000 条消息逐页可达、全文搜索与新消息锚定。这些是逻辑/操作量验证，不是 App 帧率或内存数据。

`measure-conversation-open.mjs` 读取已有数据库只读快照，使用安装的 ReactMarkdown/remarkGfm 做解析与服务端渲染。3 个会话中总文字最多的一份为 28 条/43,573 字符；优化前后分别新启动 Node 进程，初次挂载助手 Markdown 从 14 条/1,841 字符降为 6 条/893 字符。首轮 15.64ms → 18.11ms，没有提速证据；预热后 12 次中位数 2.85ms → 1.47ms。模块加载、数据库读取与浏览器布局不在计时内，不外推正式 App 性能。只保存汇总数据：[测量结果](fixtures/canvas-conversation-open.json)。

全量 TypeScript：`./web/node_modules/.bin/tsc --noEmit --incremental false -p web/tsconfig.json`；最终交接执行。没有运行共享 Next/桌面构建。

## 剩余验收和限制

主整合统一检查正式 App：长对话翻页/查找/选字/回到最新、分组跨边界拖动、裁剪/全屏/截帧、原图下载导出字节一致、恢复历史后素材重新加载、SOP 清单与用量 UI。Antigravity 真实工具调用仍未复验。

零闲置指已迁移的临时 URL；活动页面为保护原图编辑/历史而持有的字节没有硬上限，旧内联 data URL 保留原文。旧 storage-migration 在 web/src 无入口，兼容 API 保留文档退出兜底；不能宣称全应用内存已硬封顶。尚无本轮正式 App FPS、峰值内存或收费模型通过结论。
