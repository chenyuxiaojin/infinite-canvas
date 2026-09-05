# 画布独立审查证据

## 范围与结论

- 【事实】本记录只包含生产源码只读审查、内存隔离诊断、现有 Rust 单元测试及案例 4 媒体引用/文件存在性检查；没有操纵界面、改正式数据、改绑定、构建或安装 App。
- 【事实】当前 `pending-test.md` 已记载「小陈的画布 1.0.0」及左栏第 3 版完成安装，不能沿用早期“尚未构建”交接描述。
- 【事实】发现媒体显示阻断和 UI-only 持久化缺口；连续长距离平移的可见节点滞后已用真实函数隔离复现，仍需原生 App 手势确认。
- 【未知】本子审查不具备真实 WebKit 帧时间、触控板延迟、原生 App CPU/内存与多媒体解码数据，不将下述 Node 基准当作 macOS 性能验收。
- 【事实】本轮 `git diff` 中画布页面只改了默认新项目名字，store 只改默认侧栏宽度；以下画布问题的相关代码在 HEAD 已存在，不应归咎于本轮终端改造。

## 已通过的有限验证

1. 运行 `cargo test --offline --locked --lib canvas::tests --manifest-path integrations/local-agent-adapter-rust/Cargo.toml`，3 项通过：生成可编辑文本、人工锁定节点不能修改、额外路径字段拒绝。测试是内存/隔离模型，不代表真实多人共编或 GUI 验收。
2. 读取案例 4（`DUkqxVcwRh30uwMAskyxt`）的媒体元数据，32 张图对应文件全部存在，32/32 文件字节数与记录一致，总计 427,634,172 bytes。没有下载、修改或删除媒体。未做本次全文件哈希与完整解码，不能声称质量验收通过。
3. 源码确认：只有可见区域加 280 屏幕像素缓冲的节点被挂载；媒体读取最多 6 个并行 worker；图片 `decoding="async"`、`loading="lazy"`；视频 `preload="none"`，画布视频播放入口会暂停此前活动视频；低于 0.2 缩放时图片/视频替换成轻量占位。

对应源码：`canvas-client-page.tsx:905–961`，`canvas-node.tsx:693–759, 807–822`。这些是机制确认，不是 WebKit 实测表现。

## 失败/缺口

### 1. P1：案例 4 的 32 张本地图片未转成浏览器可读取的地址

- 【事实】正式 SQLite 中全部 32 张图片的 `metadata.content` 与 `storageKey` 均为 `local-ref:asset-…`，并带 `localMedia.rootId=agent-media`、`relativePath=verified/…png`。
- 【事实】文件都在，但 `prepareCanvasNodes` 只清除 `blob:` 内容（页面 5063–5069 行），不会处理 `local-ref:`；媒体 hydration 遇到已有 `content` 会跳过（927 行）；图片最终直接 `<img src={content}>`（节点 815 行）。`image-storage.ts:187–233` 也没有 `local-ref:` 解析分支。
- 【事实】主代理原生截图已看到案例 4 图片显示蓝色问号；本子审查给出与其一致的引用链根因，不把该截图当成本子代理亲自操作证据。
- 【事实】追加只读日志取证：确认 App PID 51673 与 WebKit WebContent PID 51683 的可执行路径正确；查询近 1 小时两进程包含 `local-ref`、`unsupported URL`、`unsupported protocol`、`NSURLErrorDomain` 的系统日志，结果为 0 条。另查 WebKit Networking 同时段上述协议关键词也为 0 条；应用数据目录前两层未发现 `.log` 文件。没有开启调试、改 App 或发测试网络请求。
- 【未知】未取得该 `<img>` 在真实 WebKit 中的 Network 请求记录或具体错误码，因此不能声称现场捕获了某个 `NSURLError` / HTTP 状态。当前证据是原生蓝问号 + 正式节点值 + 实际渲染源码链路；不会拿 `curl` 不支持协议替代 WebKit 失败证据。
- 【推断】缺失的是内部引用到允许读取媒体的交付链路，不是原图丢失，也不是 LOD/缩放策略。不能用 32 个加载失败占位图推断“32 张真实 4K 图片下很流畅”。
- 最小修复方向：沿受限本地媒体读取边界解析 `local-ref`，按项目/资产白名单返回可读 URL；不要把任意本地路径直接暴露给网页。随后先验收一张图的引用、字节/哈希、解码，再验收全部图片和性能。

### 2. P2：只改变视口或面板时，项目分片不保存

- 【事实】`use-canvas-store.ts:440–451` 将 viewport/sidePanel/agentPanel 识别为 UI-only，保留 `updatedAt` 并跳过 `queueProjectSave`。保留业务更新时间本身合理，但本地 UI 状态仍需要持久化。
- 【事实】`projectNeedsWrite`（172–188 行）又完全没有比较这 3 个 UI 字段。`persistLocalProjects`（217–232 行）在分片已建立后只写 dirty 项目；最后虽更新 index 和已写引用，却没有保存 UI 改动。
- 【事实】隔离运行真实 `updateProject`、`projectNeedsWrite`、`persistLocalProjects`：上述 3 种 patch 分别都是内存更新成功、桌面保存调用 0 次、项目分片写入 0 次、仅 index 写入 1 次。
- 【推断】若之后没有业务内容变化顺带保存，完整重载会恢复之前持久化的视口/面板值。与文档要求“原保存宽度不变”相关；需原生 App 在已稳定保存的隔离项目复现。
- 最小修复方向：把 UI-only 字段纳入本地分片 dirty 检测，继续保持 `updatedAt` 不变、避免账号/业务全量同步。不要为了保存面板把拖动变成全画布同步。

### 3. P2：持续平移时视口剔除使用旧状态

- 【事实】`handleViewportChange`（页面 1441–1455 行）只更新 ref，直到输入停止 80ms 或收到 immediate 才更新 React viewport。
- 【事实】`visibleNodes`（905–920 行）和 `mediaLite`（922 行）依赖 React viewport，不读取这个实时 ref。
- 【事实】隔离 60 次、间隔 16ms、累计 960ms 的持续平移：React viewport 提交 0 次。将视口从 x=0 移到 x=-800 时，原位 x=1750、宽 100 的节点已位于 1200 宽屏幕内，却仍被旧视口判定为不可见。停止 80ms 后才提交并挂载。
- 【推断】实际连续拖过 280 屏幕像素缓冲后会出现新区域暂空/停手弹入；缩放跨过 0.2 的媒体模式也会等到停手。真实触控板事件频率/停顿会影响发生程度，待 GUI 确认。
- 最小修复方向：将“渲染可见集合/LOD 的更新节奏”与“停止后持久化”分开，保证连续操作期间可见集合有有界更新，不恢复每个输入事件全树重渲染。

## 性能风险与机制核验

### rAF 只合并了通知，未合并所有 DOM 工作

`infinite-canvas.tsx:73–87` 在安排 rAF 前立即 `applyCanvasViewport`；该函数 23–46 行更新世界层 transform、CSS 变量和网格背景。隔离 100 次输入实际调用 DOM apply 100 次，只剩 1 个待处理 rAF 通知。因此文档“滚轮已合并到 rAF”不能理解为全部样式工作每帧只做一次。

### 拖一节点仍刷新全部可见连线

`canvas-client-page.tsx:1679–1685` 在每次 mousemove 时先复制全节点位置预览并做分组目标搜索，然后才合并 rAF。rAF 内 1713–1718 行遍历全部显示连线，每条连线分别在全节点中 `find` 两端，然后更新全部 path；没有只选关联连线。

以下是原函数在 Node v24.12.0、darwin/arm64 的隔离函数基准，每组 100 次。DOM 为无开销 mock，分组搜索为 stub；计时用普通 ID 属性，ID 读取次数另外单次插桩计算。数值只证明 CPU 算法增长，不是 WebKit 帧耗时，也不能给出 App FPS。

| 节点 / 可见连线 | 拖 1 节点后 path 更新次数 | 节点 ID 读取次数 | p50 / p95 / 最大（ms） |
| --- | ---: | ---: | --- |
| 48 / 47 | 94 | 2,540 | 0.042 / 0.068 / 0.902 |
| 500 / 499 | 998 | 252,496 | 2.167 / 2.276 / 2.611 |
| 2000 / 1999 | 3,998 | 4,009,996 | 32.080 / 36.464 / 42.160 |

【推断】大规模可见连线下，这部分会成为 WebView 主线程瓶颈候选；应先限制到被拖节点相连边，并用 node ID map 查找，之后再测真实帧时间。48 节点场景的这段逻辑本身很小，不能据大规模合成输入解释用户当前所有卡顿。

### 媒体内存不会随出屏自动全部释放

【事实】视口外 React 节点不挂载，但已读取的 Blob URL 保留在 image-storage 的模块 Map 中（225–232 行），并被写回节点 `metadata.content`（页面 952–955 行）。本次看到的 revoke 路径属于显式替换/删除，不是节点出屏或项目切换。可见性剔除减少 DOM/解码/绘制，不等于所有读过的原媒体立即释放。持续走访多个大图区域的内存峰值/回落仍需真实进程采样。

## Rust 与实际工作分工

| 部分 | 当前源码确认的职责 | 不应误归因 |
| --- | --- | --- |
| React / WebView | 指针事件、节点与 SVG、图片/视频、xterm、UI 状态、JSON 序列化 | 没有因外壳变 Rust 自动成为原生逐帧画布 |
| Rust / Tauri | App 生命周期、普通 PTY、固定子进程、受限 IPC、SQLite 画布适配、Bridge、本地执行器 | 不执行上述 DOM 绘制与媒体 HTML 元素渲染 |
| Node / Next | 3100 提供前端与 API 路由；桌面 API 转发目标为本机 3101 | 不是用户已部署外部云端 |
| Go API | 3101 本机业务 API / 提示词等数据服务 | 不是所有画布逐帧交互的必经环节 |

源码依据：`desktop/src-tauri/src/lib.rs:223–287`；`desktop/src-tauri/src/agent_bridge.rs:52–116`；`web/src/services/desktop-runtime.ts:103–115`；上述画布组件。

【事实】`pending-test.md` 关于 Go/Node 并行启动的旧描述与现源码不符：`lib.rs:246–275` 是先启动 Go 并等待 3101，再启动 Bridge，随后启动 Node 并等待 3100。只能标记实现/文档不符，不能声称现版已并行或量化其收益。

## 可复现入口与后续真实验收

源码隔离诊断：仓库根执行 `node docs/progress/canvas-audit-source-tests.mjs`。脚本从当前源码 AST 抽取函数，每次输出源码 SHA-256；它断言“观察到的行为”并输出每项验收状态，退出 0 仅表示诊断完成，不表示缺陷已通过。只需现有 `web/node_modules/typescript`，不安装新库。

未发现现有前端 UI/performance 自动测试入口：`web/package.json` 只有 dev/build/start/format/format:check；既有 Rust Canvas 单元测试属于协议/数据约束，不测画布帧率。

建议主代理在隔离项目按以下顺序做原生验收：

1. 放一个可正常解码的确定性本地图，确认图片显示，再加到明确数量；失败图片不能计入多媒体压力负载。
2. 对长距离持续平移（单次超过 280px）与持续缩放跨 0.2 录可见变化；停手与不停手分别检查节点进入屏幕是否缺失。
3. 保存项目后只改视口/面板，等待 2 秒，不改正文，完全重新加载，核对各值；全程仅隔离项目。
4. 分别在 48、500 个节点及有/无可见连线场景拖节点；逐级加压，不一开始向正式片子灌数千节点。
5. 记录动作窗口的 WebKit、主 App、Node、Go CPU/RSS 和真实帧/输入延迟；固定 App 版本、窗口尺寸、缩放、媒体规格和运行任务，使重测可比较。

## 已提供的原生 App 导入素材

- 文件：`docs/progress/fixtures/canvas-audit-multimedia-v1.zip`，1,757,989 bytes，SHA-256 `2bf7d8376db4906a35e23d6bf11810ac498f16a64fd84cd49f941dd297c4b83b`。
- Canvas v3 格式，标题「独立验收-多媒体-临时」，60 节点（36 图 / 12 视频 / 12 文本）、59 连线，48 个独立 `image:` / `video:` 存储键。媒体 `metadata.content` 全部省略，必须正常 hydration 后显示；不含本地任务 ID、生成请求或 `local-ref:`。
- 图片复制自 `data/p3-evidence/p3-test-image.png`（632 bytes，320×180）；视频复制自既有 `P3-workflow-bedaac2.zip` 内 MP4（138,603 bytes，320×180，1 秒，MPEG-4 Part 2 + AAC），未改原件。完整 FFmpeg 解码、导出后 48 个媒体条目字节哈希往返校验及 `unzip -t` CRC 检查均通过。
- 【未知】此 MP4 的真实 WebKit 播放兼容性要点击实测，FFmpeg 解码通过不等于 Safari 支持其编码。若播放不兼容，应另建明确衍生的 H.264 fixture，不覆盖原件。
- 【事实】重复低分辨率素材仅供“能实际加载/交互”的初级验收，解码/缓存压力有限，不能代表 4K、多来源、大项目的性能。
- 【事实】主代理回报：CUA 文件选择/管道不稳定，导入操作已取消；此 ZIP 仅已生成和离线校验，未成功导入 App。因此真实多媒体画布平移、缩放、拖节点及 CPU/RSS 压力验收仍未测，不得把 fixture 创建成功当作 GUI 验收通过。
- 重建：`node docs/progress/make-canvas-audit-fixture.mjs`。详细参数、来源、SHA-256 与编码记录保存在同目录 `canvas-audit-multimedia-v1.manifest.json`；脚本仅新建或确认相同产物，不覆盖不同文件。App 的 `importProject` 会生成新 project ID，主代理须在 CUA 导入后记录真实 ID，不直接写正式数据库。

本记录不替代主代理的 App 操作、终端与安装/数据验收报告。未改公共 `pending-test.md` / `todo.md`，由主代理统一收敛状态。
