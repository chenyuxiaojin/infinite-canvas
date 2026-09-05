# 本地原图显示修复（第 1 步）

## 范围与交付状态

用户已授权按顺序修复：图片显示 → 旧路径与残留登记 → 终端输出。首轮源码专项仅实现图片显示及针对性测试，没有提交、打 tag、打包安装或操作正式画布界面，没有修改原图、正式节点、真实片子绑定、终端或应用登记；打包和原生复测由主任务随后统一执行。

【最终状态】主任务已安装正式「小陈的画布 1.0.0」。原生抽验看到杰克、乔治、格拉迪丝及街头、酒馆、厨房、废墟原图，杰克放大预览通过；32 张全部通过受限读取/哈希/完整解码，但未声称逐张原生视觉通过。最终原图和节点/连线未变，案例 4 只有查看位置及更新时间变化。安装与具体边界见 [顺序修复验收](ordered-repair-acceptance.md)。

【事实】旧显示链路直接把 `local-ref:asset-…` 当作 `img src`，已有 content 又跳过 hydration。32 个原图实际位于应用数据目录的 `agent-media/verified`，不是片子目录下；主任务已明确确认沿用这一实际位置，不搬动原图。

## 实现边界

- Rust 新增 `read_canvas_local_image(projectId, storageKey)`，返回原始二进制字节。前端不能指定路径或根目录；正式项目的节点登记决定素材 key、相对路径、类型、大小和哈希。
- 共享物理根为 Tauri 解析的 App 数据目录下 `agent-media`。授权隔离依据当前正式项目内已登记素材，以及 AI编导直接子目录中准确保存的同一画布 ID 绑定；不按标题、不调用 setup、不修改配置。未知/冲突登记、缺失/重复/错目录绑定均拒绝。
- 复用 `AllowedRoot / ScopedPath / PathPolicy`，再以逐级文件描述符和 `O_NOFOLLOW` 打开，拒绝路径穿越和软链接；最终必须是普通文件。单图最大 64 MiB，读取增长也有上限，真实字节数及 SHA-256 必须等于登记。仅允许 PNG/JPEG/GIF/WebP 栅格图片，检查文件签名与登记 MIME 一致；签名不是完整解码证明。
- Tauri 只增加这一命令的既有本机页面权限。使用 [Tauri 二进制响应](https://v2.tauri.app/develop/calling-rust/#returning-array-buffers)，不新增 HTTP 服务、全盘文件协议或 JSON 数字数组传图。
- 前端真实图片节点与当前放大预览共用按 `projectId + storageKey` 区分的临时地址引用。最多两个同时读取；节点还按实际屏幕交集加载，低缩放占位不读取，不预加载整个画布或整个预览组。
- 最后一处显示卸载即释放 Blob URL；尚未开始的排队读取取消，已经开始的 IPC 不能中断，但完成后若无使用者直接丢弃字节、不创建 URL。项目/素材切换后的过期结果不会写入新显示。
- 原始 `content/storageKey/localMedia` 保持不变，local-ref 明确排除旧 hydration，不写入节点、全局永久图片缓存或 IndexedDB。失败显示中文原因，不用伪造缩略图掩盖错误；放大预览使用同一原文件，不生成替代图片。

## 测试记录

- 前端：仓库根运行 `/usr/local/bin/node --test docs/progress/canvas-local-image.test.mjs`，32 项通过、0 失败。测试加载真实 TypeScript 服务与 hook，注入 IPC/URL/React 生命周期替身，覆盖两并发、节点/预览共用、项目隔离、排队取消、迟到结果丢弃、卸载释放、非法字节与格式、无自动重试、屏幕交集/禁用开关以及 local-ref 不进入持久化 hydration。不是 React DOM 或原生 WebKit 实测。
- Rust：`cargo test --offline --lib`，22 项通过、0 失败、1 项显式 opt-in 忽略；其中新增图片安全测试 12 项、既有回归 10 项。包括准确/重复/缺失/错目录绑定、未知或跨项目 key、冲突登记、图片类型/签名、大小/哈希不符、真实读大小限制、目录/文件/绑定软链接拒绝、打开描述符后的路径替换，以及数据库/绑定无写入。
- 真实案例 4：显式运行 opt-in 测试，通过本次实际 `read_registered_image` 核心逐个读取 32 张原图，合计 427,634,172 bytes，每张与登记大小、SHA-256、PNG 签名一致。正式 App 素材只读；数据库使用上一轮 `post-test.db` 的独占临时副本，未将 adapter 打开在正式数据库。源快照、数据库副本、准确绑定及项目文档前后保持相同。
- Rust 原始日志：`/tmp/canvas-local-image-backend-qa.AGlp0S/rust-lib-tests.log`；真实 32 图逐项大小/哈希：`/tmp/canvas-local-image-backend-qa.AGlp0S/real-case4-image-read.log`。真实专项约 34 秒为 debug 核验总时长（含重复哈希等），不是 UI 加载时间或帧率。
- `git diff --check` 通过。本轮没有模型调用、UI 操作、正式数据或片子配置写入。
- 主任务另对同一 32 张正式原图执行 FFmpeg 严格完整解码检查（`-v error -xerror`、输出至空目标），32/32 通过，没有生成或覆盖图片。这个补充证明文件可被解码，但不能代替安装后 WebKit 窗口显示验证。

全量 TypeScript 检查仍是此前四个未改动文件的八处错误：canvas-resource-references 4 处、video-settings-panel 2 处、gemini 正则目标 1 处、canvas-agent 消息类型 1 处。本轮图片文件未新增该检查的错误；不能宣称全量类型检查通过。

## 改动文件

- 后端：`desktop/src-tauri/src/local_image.rs`（新增）、`src/agent_bridge.rs`（读取 adapter 的内部 accessor）、`src/project_binding.rs`（只将默认片子根函数开放给同 crate）、`src/lib.rs`（注册）、`permissions/desktop-runtime.toml`（此命令权限）、`Cargo.toml` / `Cargo.lock`（已锁定 libc 直接依赖及仅测试的 rusqlite）。此处缩写路径均相对 `desktop/src-tauri/`，未改标题匹配或终端逻辑。
- 前端：`web/src/services/canvas-local-image.ts`（新增）、`web/src/app/(user)/canvas/hooks/use-canvas-image-source.ts`（新增）、同画布目录的 `components/canvas-node.tsx` 与 `[id]/canvas-client-page.tsx`。保留这些文件原先无关的未提交修改。
- 测试和记录：`docs/progress/canvas-local-image.test.mjs`（新增）、本报告、`pending-test.md` 和 `todo.md`。未改写历史独立验收结论；后续安装及原生结果单独列明。

## 未验证与非本轮范围

- 【已测/未知】正式 IPC 与 WebKit 原图/放大显示已抽验；32 张逐张原生视觉、离屏内存回落和交互帧时间尚未完整覆盖，不以隔离测试替代这些原生验收。
- 两并发限制同时读取，不是全部可见原图总内存的硬上限；多张大图同时可见时仍可能占用较多解码内存。未声称解决全部画布卡顿。
- 裁剪、参考图栏、下载/导出等其他直接消费 `metadata.content` 的功能没有在本轮扩展为 local-ref 读取，不能因节点和放大预览恢复而宣称它们已通过。
- 未修改原有打开项目更新 `updatedAt` 的行为；主任务原生复测时需区分既有打开行为与素材记录变化。
- 真实 MCP 旧路径、标题兜底覆盖风险、应用登记和终端背压由后续步骤处理，本轮不交叉修改。
