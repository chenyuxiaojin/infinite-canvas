---
title: 人与本机 Agent 共用无限画布总装 Handoff
description: 三分支总装决策、公共接口、真实验收证据与发行接线
---

# 人与本机 Agent 共用无限画布总装 Handoff

## 分支与组成

- 总装分支：`feat/human-agent-canvas-integration`
- 基线：`feat/macos-director-console` 的 `e00acb7`
- 核心协议来源：`bf3e2f2022ffcb77bfd0aebae955ec378d9b9c0f`
- 本机 Agent 适配来源：`6e4af0903957af3ce09e423a8f299657589a6210`
- 人机共编 UI 来源：`1354555a76d810296b898b486b4cb22ef5bd630b`
- 独立 worktree：`infinite-canvas-worktrees/human-agent-integration`
- 本机媒体引用增量分支：`feat/local-media-reference-streaming`
- 本机媒体引用增量基线：`88a827274f80dfaaf97fc1b755088f18d4c01416`
- 本机媒体引用验收 worktree：`infinite-canvas-worktrees/local-media-reference-streaming`

最终提交以本分支 HEAD 为准；不在本 worktree 合并回主线。

## 唯一公共接口

唯一画布修改语义是：

```text
web/src/app/(user)/canvas/protocol/canvas-operation-protocol.ts
  migrateCanvasProject
  applyCanvasOperationBatch
  buildCanvasStructureOperations
  rebindCanvasProjectIdentity
```

所有调用者都修改同一 `CanvasProject`：

- 人工 UI：`useCanvasStore.updateProject` 把结构差异转为 `actor: human` 批次；
  标题、锁和撤销也进入公共协议。
- 内置 Agent：动作入口调用 Store 的 `applyOperationBatch`。
- 外部 CLI/Bridge：`CanonicalCanvasAdapter` 把白名单操作映射成
  `CanvasOperationBatch`，调用固定 loopback Next 内部端点；`project.create`
  也用同一 reducer 从 revision 0 创建 revision 1 的 SQLite 工程。
- 本地任务：发起为 human/agent `task.start`，执行结果为 system
  `task.update`，可在同批回填节点。
- 受控视频摄入：Bridge 只接收应用私有 inbox 内的 MP4 文件名和小写 SHA-256，
  在同一工程先创建 `video` 节点与 canvas task，再交给现有 Rust executor；
  ffprobe 结果由同一个 system 批次直接回填 task 终态以及节点的稳定
  `local-ref:` 资产引用、尺寸、时长和摘要，不建立第二套媒体画布，也不要求 UI 已打开。
- 白名单节点扩容：`create_image_node`/`create_video_node` 只接受已验收的受控
  `local-ref:` 引用（Bridge 预检资产真实存在），`create_config_node` 只收
  model/size/count；协议映射仍是外部命名到公共 `node.create` 的转换。
- 受控图片摄入：`POST /v1/media/image-ingests` 沿同一 inbox basename + SHA-256
  边界收 `.png/.jpg/.jpeg/.webp`（上限 100 MiB），校验、内容寻址拷贝与 ffprobe
  尺寸探测同步完成后，用单个 agent 原子批次直接建成品 `image` 节点；不产生
  canvas task，Bridge 响应不含播放 URL，重放同一请求在 inbox 清理后仍幂等。
- 本机已有素材：工程只保存 root ID、根内相对路径、SHA-256、媒体元数据和稳定
  asset/storage key；临时播放 URL 和随机凭据只存在当前 App 进程内，不进入
  `CanvasProject`、Agent 上下文、ZIP 清单或日志。

在包含本总装代码的桌面包内，持久化唯一来源是 SQLite
`canvas_projects.project_data.operationState`，其中包含数值 revision、locks、
tasks、requests 和 audit。WebView 通过 Tauri IPC 读写这些行并轮询当前打开
工程；内部 Next 端点是无状态 reducer 执行器。浏览器独立构建和旧桌面包仍会
使用 IndexedDB，不能把“隔离验收包通过”解释成旧桌面包已经自动获得 Bridge。

## 冲突决策与删除项

### UI 冲突

- `canvas-client-page.tsx` 保留原交互与 React 本地渲染状态，但外部工程版本
  水合期间设置同步栅栏，禁止旧节点视图回写成“人工删除”。
- `use-canvas-store.ts` 保留 IndexedDB/账号同步兼容路径；桌面合并先比 revision，
  revision 相同才比 `updatedAt`，无差异 patch 不再制造保存。
- 画布库明确显示“本机数据库与 Agent Bridge 已连接”；Tauri IPC 补水或保存失败
  会显示错误状态，不再只写浏览器控制台后静默表现成可供 Agent 使用。
- `canvas-collaboration-adapter.ts` 只从 `operationState` 映射状态、历史和节点标记；
  不拥有 reducer、revision、锁或 undo 快照。
- system 对 Agent 媒体批次的 runtime task ID、进度和验收结果回填不会使该 Agent
  批次失去可撤销性；任何后续 human/agent 结构修改仍会阻止旧批次撤销。
- UI 打开 headless 已完成的视频节点时先把旧 `local-task:` 迁移为受控
  `local-ref:`；后续水合只在内存中补入带随机能力凭据的 loopback 播放 URL，
  URL 不参与协议差异比较，也不会因滚动、缩放或播放制造 revision。
- 桌面画布库每秒从同一 SQLite 刷新，因此 CLI 新建工程无需重载或重新导入即可出现。

### Bridge 冲突

- 删除 `SqliteCanvasAdapter` 的独立 reducer、SHA revision、锁判定与 SQL journal。
- 删除 `agent_operation_requests` 建表和写入；幂等完全来自
  `operationState.requests`。
- Bridge 仍保留稳定 HTTP/CLI JSON、凭据、退出码和白名单能力；协议映射只做
  `create_text_node` 等外部命名到公共 operation 的转换。
- `save_human_project` 以 revision 为第一优先级，只有 revision 相同时才比较墙钟，
  避免较旧时间戳拒绝更高 revision。
- Next loopback reducer在 WebView 导航瞬间出现传输失败时最多重试 2 次；request ID
  使响应丢失后的重放仍然幂等，不会重复建节点或启动任务。

### 兼容与恢复

- 旧工程缺少 `operationState` 时迁移为 revision 0，不改节点和连线。
- 本地媒体持久化为稳定 `local-ref:` storageKey；`blob:`/`data:`、loopback URL 和
  `localPlaybackUrl` 均为运行态，不计为修改，也不能进入 Agent 上下文。
- ZIP v3/v4 继续导入，当前原生导出为 v5。v5 可明确选择“嵌入媒体”或“仅引用/清单”；
  前者按 SHA-256 写入受控项目副本，后者保留引用并在缺失时进入 structured
  `missing/relink`，不会静默丢文件。导入副本换 ID 时仍重绑定 audit/result/request
  指纹并保留锁、task、历史、连线和幂等。
- 旧 Blob 工程继续走浏览器兼容路径；旧 `local-task:` 工程由桌面引用命令迁移，
  原来的整段字节 IPC 已删除。

## 安全边界

- 正式端口固定 `127.0.0.1:3100/3101/3102/3103`；验收 feature 使用独立固定
  `127.0.0.1:3210/3211/3212/3213`。3103/3213 是随机能力凭据保护、支持 HTTP
  Range 的媒体流，只监听 IPv4 loopback，不接受动态 host 或公网监听。
- Bridge 不开放 shell、可执行路径、任意路径、任意 URL、raw SQL 或付费生成。
- Agent 不能提交任意路径：`GET /v1/media/inbox` 只返回应用私有固定 inbox，摄入
  只接受受控 asset ID、inbox basename 或已授权 root ID 内相对路径；绝对路径、
  目录穿越、符号链接越界、摘要不匹配、空文件和超过 1 GiB 均结构化拒绝。
- 当前 Developer ID 非 Mac App Store 架构使用应用私有 root registry；只有用户通过
  原生选择器选中的根或应用自有项目媒体根会写入 registry，文件权限为 0600。
  若未来切换 App Sandbox，root registry 必须替换为 security-scoped bookmark，
  不能降低成任意绝对路径。
- 安装凭据只写应用支持目录私有文件；CLI 不接受 token 参数。
- Tauri capability 只放行固定 WebView origin 和必要的桌面画布命令。

## 本机媒体引用与 Range 流

桌面端选择固定 loopback HTTP，而不是 Tauri 自定义协议。原因是 WKWebView 的原生
`<video>` 对标准 HTTP Range、`206 Partial Content`、`Content-Range` 和媒体 seek
的行为更可验证；同时固定 loopback 可在 Rust 单测里完整覆盖鉴权、Origin、Range
和 416，而不需要把文件读成 WebView `Uint8Array` 或 Blob。

受控数据流如下：

```text
用户原生选择器 / Agent inbox basename
  -> Rust 校验 root + 相对路径 + symlink + 大小 + SHA-256
  -> CanvasProject.metadata.localMedia (稳定、无绝对路径)
  -> App 进程内注册 asset ID
  -> http://127.0.0.1:3103/v1/media/<asset-id>?token=<随机能力凭据>
  -> WKWebView <video preload="metadata"> 发起 Range
  -> Rust 流式读取请求区间并返回 206
```

- “引用本机素材”是默认模式，不复制、不上传；“复制进项目”是可移植工程/备份/完整
  ZIP 的明确选择；“上传至云存储”仍是另一个明确动作，默认不发生。
- `LocalMediaReference` 的公共字段为 `storageKey/assetId/rootId/relativePath/sha256/
  mimeType/bytes/fileName/width/height/durationMs/mode`。运行态 `playbackUrl/status/reason`
  只供当前 WebView，持久化前会被规范化掉。
- `missing` 与 `digest_mismatch` 都不会自动猜路径；UI 显示“已移动或不可用”，只能由
  用户从原生选择器重新定位。relink 会重新执行根、符号链接、摘要与大小校验。
- 服务凭据每次进程启动随机生成；请求必须同时满足能力凭据、固定 asset ID、已注册
  引用和允许的 loopback Origin。记录的验收证据不含能力凭据或绝对路径。

## 真实零付费验收

以下结果来自隔离 bundle `com.chenyuxiaojin.infinitecanvas.integrationtest`，证明
总装代码的数据流成立，但不证明基线主检出生成的旧 `无限画布.app` 已包含这些
代码。正式 bundle 的升级迁移必须单独验收，不能用本节代替。

复现配置：

```bash
cd desktop
PATH=/Users/chenhuajin/.cargo/bin:$PATH \
  bun run tauri build \
  --config src-tauri/tauri.integration.conf.json \
  --bundles app \
  --features integration-acceptance
```

隔离 bundle ID 为 `com.chenyuxiaojin.infinitecanvas.integrationtest`，不会读写
正式 App 的项目数据。真实结果：

1. UI 生成固定 1 秒本地测试片；human 创建节点/连线和 task，system 回填成功。
2. 打包 CLI 读 capabilities/工程；dry-run 不落库；apply 创建 Agent 节点/连线；
   已打开 UI 自动显示；同 request 重放不重复。
3. UI 锁定后 CLI 返回 `LOCKED_NODE`；人工 revision 前进后旧请求返回
   `STALE_REVISION`；重读后可修改未锁节点。
4. UI 撤销最近 Agent 批次；重启后 revision 10、锁、task、audit 和结构仍在。
5. 原生保存框导出并重新导入；副本保留 4 节点、2 连线、1 锁、1 task、
   12 audit，历史身份与幂等已重绑定到副本 ID。
6. 监听仅 loopback；标准 App 内 CLI 与 release CLI 哈希一致；真实凭据内容未在
   tracked files 或标准 App 可执行文件中出现。
7. 新增媒体增量在同一隔离 bundle 完成：CLI 在已打开画布库时创建工程，卡片无需
   重载即出现；固定 inbox MP4 经 dry/哈希边界后生成 640x360、2005 ms 视频节点，
   UI 自动出现并可播放。项目稳定在 revision 4，只有 1 个 node、1 个 canvas task、
   4 条 audit；重放同 request 后四项计数均不变。
8. 人工锁定视频节点后 CLI 得到 `LOCKED_NODE`；人工解锁使 revision 5→6 后，旧
   base revision 得到 `STALE_REVISION`。应用退出重启后 revision 6、节点、任务、
   审计和媒体仍在；重复摄入返回 `duplicate:true` 并报告当前 revision 6。
9. 真实导出 `/tmp/agent-media-final-export.zip` 为 514 KiB，包含 `projects.json`
   与 491694 bytes 的 MP4。首次回灌暴露 WKWebView 原生选择框把组合 MIME/后缀过滤
   误判为不可选；输入过滤收窄为 `.zip` 后重包复测，Open 恢复可用并成功导入副本
   `waCbl0WxWvM8Ro1OjHYYa`。副本为 revision 6、1 视频节点、1 task、8 audit，历史
   project ID 全部重绑定，UI 仍显示 00:02 并可播放。
10. 独立撤销工程的 Agent 媒体批次在两次 system 回填后仍显示“完成 · 可撤销”；
    用户确认后从真实 UI 点击“撤销批次”，UI 变为“画布元素 0 / revision 5 /
    已撤销”。CLI 同步读回 0 node、0 connection、0 task、5 audit，原 ingest audit
    记录非空 `undoneByRequestId`。
11. 正式 App 的一次性工程 `冒烟-一次性工程（可删）` 暴露旧完工批次只有
    `task.update` 的 headless 缺口；打开 UI 后节点由 loading 恢复为 00:06 可播放，
    实际点击进入“暂停”状态，证明旧 UI 兜底有效，但也证明 CLI-only 不能算完整。
12. 修复后在全新隔离工程 `headless-h264-norev-20260831` 复测：工程从未在 UI
    打开，CLI 轮询任务后 SQLite 已为 revision 4、节点 `success`，最后一个 system
    批次同时包含 `task.update + node.update`，并具有稳定 content/storageKey、
    1344x768、6583 ms 和摘要；重复轮询不增加 revision。随后 UI 直接显示 00:06，
    点击进入“暂停”，CLI 再读仍为 revision 4、4 audit。H.264/AAC 输入 255441 bytes，
    stream-copy 输出 254832 bytes，ffprobe 与完整 `-xerror` 解码通过。
13. 27 镜性能样本使用原始 17 MiB 审片 ZIP，只导入隔离 bundle；工程
    `L2DupmT6GUhE_2K4SUCNK` 为 40 节点（27 个真实 MP4）/27 连线。UI 全部显示
    00:06 元数据，真实 S18 节点进入“暂停”状态；6 轮侧栏往返滚动和 6 轮
    27%/68%/约 120% 缩放没有白屏、崩溃或结构丢失，CLI 前后均读回 revision 1、
    40 节点、27 视频和 27 连线。四个相关进程的 RSS 从画布库 1,063,232 KiB 增至
    打开稳定后的 1,197,040 KiB，滚动/播放后约 1,209,920 KiB，缩放压力后稳定在
    1,303,968 KiB；其中 WebContent 从 785,024 KiB 增至 1,012,336 KiB。退回画布库
    10 秒后总 RSS 仍为 1,307,648 KiB，没有回收这约 239 MiB 增量；系统内存空闲
    73%，未崩溃。退出隔离 App 后四个进程退出，3210/3211/3212 全部关闭。
14. 本机媒体引用增量使用全新隔离 bundle
    `com.chenyuxiaojin.infinitecanvas.localmediaacceptance`，避免复用上一轮仍含 27 个
    Blob 的 WebKit 容器。原始 ZIP 保持只读；验收只使用一次性副本，二者均为
    17,965,118 bytes、SHA-256
    `f9d0e2eead775229365107bd87289ba8ebe47d072e62c92594c11df0d55d4a93`。
    第一次导入生成 27 个内容寻址项目媒体文件；相同 ZIP 再次导入时应用支持目录仍为
    18,252 KiB，证明相同摘要没有制造第二套媒体副本。
15. 最终工程 `h-8xz69xIDvxhphOd4JK1` 为 40 节点、27 视频、27 连线、27 个
    `project_copy` 本机引用；持久化检查为 `blob:` 0、`local-task:` 0、运行态播放 URL
    0、绝对路径字段 0。旧 v3 工程首次规范化后 revision 为 1；实际播放、滚动、
    缩放、返回画布库后仍为 revision 1、audit 1。脱敏面板记录 54 个 Range 请求，
    54 个均返回 206；截图保存在忽略目录
    `data/local-media-reference-evidence-20260831/range-panel-metadata-54x206.jpeg`。
16. 同一最终进程的画布库基线总 RSS 为 952,240 KiB，27% 打开稳定后为
    1,115,216 KiB（+162,976 KiB）。包含三段实际播放和两向滚动的另一轮峰值为
    1,455,152 KiB；不同重启的 WebContent 基线波动很大，因此只报告同进程差值，
    不把它换算成 FPS。离开页面 10 秒为 1,115,456 KiB，没有稳定回到基线；视频
    卸载已显式 pause/remove-src/load，但 WKWebView RSS 仍保留，这是残留风险而非
    性能通过。最终退出后 App/Go/Node/WebKit 验收进程均退出，3210–3213 全部释放。

## 用户现场复核与交付断点

用户已在 2026-08-30 完成正式 bundle 的安全换装：先成对备份 Application
Support 与 WebKit 到
`~/项目/自己的应用/infinite-canvas-backups/backup-20260830-1815.tar.gz`，再启动
`~/Applications/无限画布.app`。现场确认 3100/3101/3102 均正常、未授权 Bridge
访问为 401、CLI 可列出迁移后的 4 个工程，并在一次性 P3 工程完成 dry-run、apply、
revision 0→1、幂等、审计与撤销快照。也就是“桌面 SQLite + Bridge 直写”已经由
用户实机复核，不再是旧包/空表状态。

正式 App 已完成受控 MP4 冒烟；用户确认后，`冒烟-一次性工程（可删）` 已从真实
画布库删除，255441-byte inbox 副本已移入废纸篓，可恢复。没有向其他正式工程追加
视频或任务。headless/H.264 修复和 27 镜性能测试只写独立 bundle
`com.chenyuxiaojin.infinitecanvas.integrationtest`。最终标准 App 会构建并暂存，不在
正式进程运行时覆盖安装。

本机媒体引用正式换装步骤：

1. 退出正式 `无限画布.app`，确认 3100–3103 均未监听。
2. 成对备份
   `~/Library/Application Support/com.chenyuxiaojin.infinitecanvas/` 与
   `~/Library/WebKit/com.chenyuxiaojin.infinitecanvas/`；不要只备份其中一个。
3. 用本分支标准构建产物替换 App 本体，不删除或移动上述数据目录。
4. 首次启动只打开副本工程，确认 3100–3103 仅监听 `127.0.0.1`、旧工程可见、
   `local-task:` 能迁移、missing/relink 文案和 Range 播放正常。
5. 用“仅引用/清单”和“嵌入媒体”各导出一个测试 ZIP 并回灌；确认无误后再打开正式
   用户工程。若失败，退出 App、恢复旧 App 本体；只有数据迁移不兼容时才成对恢复
   Application Support 与 WebKit 备份。

## 验证命令与结果

```bash
go test ./...

cd web
bun run test:canvas-protocol
bun run test:collaboration
bun run build

cd integrations/local-agent-adapter-rust
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets

cd ../../desktop/src-tauri
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets --all-features

cd ..
PATH=/Users/chenhuajin/.cargo/bin:$PATH bun run tauri build --bundles app

PATH=/Users/chenhuajin/.cargo/bin:$PATH \
  bun run tauri build \
  --config src-tauri/tauri.local-media-acceptance.conf.json \
  --bundles app \
  --features integration-acceptance
```

最终硬门禁计数：Web 协议/Store/共编/本地媒体共 26 tests，本机 Agent crate
7 unit + 10 contract tests，桌面 crate 17 tests，本地 executor 19 tests（另有
1 个需显式 trusted FFmpeg 的测试默认 ignored）。Go 全部 package、Next 生产构建、
标准 arm64 `.app` 与独立本机媒体验收 `.app` 均构建通过；真实 H.264 headless、
v3/v4 迁移、v5 两种导出、missing/relink 和 HTTP Range 闭环已覆盖。标准包与验收包
内 CLI SHA-256 均为
`9d43d06bd3d8ffb7a1de1d736beb1d1dbdc7f4bfb9001c1e0d6dd84f803af57c`。

`bun x tsc --noEmit` 仍有 8 个基线错误，位于
`canvas-resource-references.ts`、`video-settings-panel.tsx`、`gemini.ts` 和
`canvas-agent.ts`；这些文件未因总装修改，Next 构建不受影响。

## 残留风险与发行接线

- 当前标准 `.app` 是技术构建；Developer ID、公证、staple 和干净机升级仍按
  P4 矩阵执行，不能把 ad-hoc 包当发行包。
- 用户当前正式包已包含受控视频增量，但仍是“UI 打开后补回填 + MPEG-4 Part 2
  重编码”的旧实现；headless/H.264 修正版不能在运行中覆盖，需用户退出后换装。
- 27 个真实 MP4 已不再持久化或恢复成 WebView Blob；受控 loopback Range、稳定
  `local-ref:`、引用/复制选择和 v5 导出均已落地。剩余性能风险是 WKWebView 即使在
  `preload="metadata"`、卸载时显式释放 `<video>` 后，路由离开 10 秒的 RSS 仍没有
  稳定回到画布库基线。它不是整段 Blob 副本证据，但也不能宣称性能完全通过；后续应
  增加帧时间与内存压力仪表，并评估视频节点虚拟化/持久缩略图，而不是恢复 Blob。
- 发行签名后再次核对 CLI 仍在 `Contents/MacOS/infinite-canvas`、所有内嵌
  Mach-O 的签名顺序、Bridge loopback 监听和凭据文件权限。
- 全仓 TypeScript 基线修复可另开任务；不应在本总装分支混入无关业务修正。
