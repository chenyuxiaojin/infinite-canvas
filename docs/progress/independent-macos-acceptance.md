# 小陈的画布 1.0.0 独立 macOS 验收

## 结论

【事实】整体验收不通过。有限终端输出、程序注入输入、部分会话回收和原数据库保护有实测通过证据；图片显示、真实片子 MCP 旧路径和终端超量积压仍有明确问题。没有证明全部卡顿解决，也没有测得画布 FPS。

【事实】接手时新版已安装，本轮未重复构建安装、未退出主 App、未强杀进程、未改生产源码/认证/API Key/Bridge 鉴权/全局 Agent 配置、未提交或打 tag，没有模型生成请求。仅新增测试、报告、快照和明确隔离的空测试项目。

## 验收对象与证据边界

- 真仓库：`/Users/chenhuajin/项目/自己的应用/infinite-canvas`；HEAD `5d8eea13b415ff86062670a03cbffed4866162e0`，原有未提交改动保留。
- 正式 App：`/Users/chenhuajin/Applications/小陈的画布.app`；界面与 plist 均为 1.0.0；identity 仍为 `com.chenyuxiaojin.infinitecanvas`。
- 主二进制 SHA-256：`641c116d410ae32dc9c607c21a7afc4216542e17f8a917e722840a00048c21aa`。既有构建清单 247 个源文件哈希与当前一致。
- 环境：macOS 26.6.2 / 25G83，Mac16,6，36 GiB 内存、14 逻辑 CPU。主 PID 51673 全程未变。
- 原生 UI 操作全部经 CUA；它们是自动化操作，不是用户物理键鼠测量。PTY 探针在正式 App 的普通终端内运行，无模型/网络。
- DSR 是输出经 PTY/Tauri/xterm 解析后答复回到 PTY的往返，不等于绘制完成、FPS 或逐键到屏幕延迟。
- UTF-8 全边界、1000 行逐行校验、50 MB 积压故障与画布函数诊断是隔离进程测试；未向正式 App 注入 50 MB。
- 旧版基线只有 Git 源码和已存在的包/数据备份，没有重新运行旧 App，所以没有可靠的新旧提速比例。

## 通过的有限项目

| 项目 | 证据 | 边界 |
| --- | --- | --- |
| 名称/签名/运行 | 原生 1.0.0、严格本机签名、3100/3101/3102 服务正常 | 不是发行签名、公证或本轮安装回滚演练 |
| 普通终端 | 普通命令可执行，原生截图显示中文/emoji 和编号输出；无自动 AI 启动 | 未逐字导出整段原生滚动缓存 |
| 二进制正确性 | 单一 Channel；真实 xterm 的 34 个 UTF-8 分割边界无损；1000 行一致、EOF 一次 | 完整逐字断言来自隔离 JS 测试 |
| 有限输出 | 6 轮生产 31,457,118 字节，62 次 DSR，0 超时/异常 | 最大单轮约 8 MiB，最长 15 秒 |
| 输出中输入 | 512 KiB/s 输出期间，typeText 的 A/B/C/回车各到达 PTY 一次 | 程序注入，不是物理键盘延迟 |
| 部分生命周期 | 改标题/切主题保持 PID 58797；5 轮开关后只剩当前 Shell；离开画布后测试进程消失 | 不覆盖所有迟到启动/异常/复杂前台进程树 |
| 临时 MCP | 显式绑定新目录，STDIO 初始化、4 工具发现、canvas_context 成功返回指定空画布 | 不表示真实三个片子的旧配置已修复 |
| 数据保护 | 原 9 行画布全部列与 JSON 相等，其余 18 表双向零差异 | WebKit/IndexedDB Blob 未逐项验证 |

## 原生终端测量

| 场景 | 输出端生产耗时 | 解析往返 |
| --- | ---: | ---: |
| 1 MiB 第一次 | 45.95 ms | 尾部 126.41 ms |
| 1 MiB 第二次 | 13.09 ms | 尾部 70.09 ms |
| 8 MiB | 114.29 ms | 尾部 601.64 ms |
| 15 秒、512 KiB/s，第 1 次 | 15.00 秒 | 最大 17.95 ms |
| 15 秒、512 KiB/s，第 2 次 | 15.00 秒 | 最大 20.87 ms |
| 10 秒、512 KiB/s，输入 ABC+回车 | 10.00 秒 | 输入回显后 2.34–20.07 ms |

【事实】8 MiB 窗口，主 App / WebContent CPU 采样峰值为 127.2% / 120.4%，RSS 峰值约 139.1 / 714.9 MiB；15 秒流式时主 App CPU 峰值约 13–15%，WebContent 约 40–42%，Go/Node 采样为 0%。CPU 超过 100% 是多核口径，不是系统总使用率。混合采样 WebContent RSS 最高 770.4 MiB；它也承载画布和缓存，没有长期回落测试，不能认定内存泄漏。

【推断】本次终端负载主要落在 Rust 传输/调度与 WebView/xterm 解析。画布节点、SVG、图片/视频仍由 React/WebView 绘制，Rust 外壳不会自动消除这些工作。8 MiB 洪峰已有约 0.60 秒尾部解析等待，不能说所有大输出无延迟。详见 [终端原生实测](terminal-native-audit-evidence.md)。

## 失败和最小修复建议（未实施）

### 图片显示失败

【事实】案例 4 在 100% 缩放显示蓝问号；32 个原文件均存在、字节数与记录一致，总计 427,634,172 bytes。正式节点 content/storageKey 是 `local-ref:asset-…`，页面未转换，已有 content 又跳过读取，最终直接作为 img src。

【未知】窄范围查询 App/WebKit 近一小时日志无相关条目，尚未取得真实 Network 请求记录或具体错误码。证据是原生显示失败、正式节点值与渲染源码链路，不能声称捕获了特定 HTTP/NSURLError。建议沿既有受限媒体读取边界解析引用，先核验一张，再测全部；不放开任意文件路径。见 [画布证据](canvas-audit-evidence.md)。

### 六处真实片子 MCP 路径失效

【事实】以下文件的 infinite-canvas 命令仍指向不存在的 `/Users/chenhuajin/Applications/无限画布.app/Contents/MacOS/infinite-canvas`：

- `/Users/chenhuajin/项目/视频制作台/AI编导/案例2-美甲师日常/.mcp.json`
- `/Users/chenhuajin/项目/视频制作台/AI编导/案例2-美甲师日常/.codex/config.toml`
- `/Users/chenhuajin/项目/视频制作台/AI编导/案例3-国运末世/.mcp.json`
- `/Users/chenhuajin/项目/视频制作台/AI编导/案例3-国运末世/.codex/config.toml`
- `/Users/chenhuajin/项目/视频制作台/AI编导/案例4-克兰奇杀妻案/.mcp.json`
- `/Users/chenhuajin/项目/视频制作台/AI编导/案例4-克兰奇杀妻案/.codex/config.toml`

全局 CLI 链接正确，不能补救这些绝对路径。三片子的 9 个绑定文件前后哈希均未变。建议经授权逐目录备份、使用既有绑定流程更新路径并保持原画布 ID。案例 4 有两个同名不同 ID 画布，应依已保存绑定识别；标题兜底还存在覆盖已有其他 ID 绑定的风险。见 [安装/绑定证据](installation-data-audit-evidence.md)。

### 输出积压超限后 Channel 停止前进

【事实】隔离测试用真实 xterm/Channel，在待解析 50,003,968 字节时抛出 `write data discarded, use flow control to avoid losing data`；回调异常导致序号停止，后续数据/EOF 不交付，排空后仍不恢复。建议以 write 完成回调确认消费，限制未处理字节，并在关闭/异常时释放等待、明确结束会话；只吞异常会丢输出。

最小复现：仓库根运行 `/usr/local/bin/node --test docs/progress/terminal-audit.test.mjs`。第 4 个“通过”是成功复现故障，不是产品通过。见 [终端故障证据](terminal-audit-evidence.md)。

### 其他已定位缺口

- 【事实】隔离真实函数：单改视口/侧栏不写项目分片或桌面库；连续 960 ms 平移可见集合不提交，等停手更新。完整重载、长距离手势表现待原生验证。
- 【事实】拖一个节点仍扫描所有可见连线并在全节点查找，计算量随规模增长；隔离基准不是 App 帧时间。相关画布代码在 HEAD 已存在，不是本轮终端回归。
- 【事实】当前源码先等 Go 再启动 Node，与旧文档“并行启动”不符。
- 【事实】正式位置、Spotlight、CUA 系统应用枚举仅一个画布，无独立 Node/next-server 应用；但 Launch Services 仍有两个废纸篓旧包记录。“唯一正式安装”通过，“全部登记只一条”不通过。未清理登记或废纸篓。

## 未完成与自动化限制

【事实】CUA 出现 native pipe closed、not active、windowNotFoundAtPosition 以及缩小/陈旧截图。裸字母 pressKey 曾仅进入隐藏输入框、未到 PTY；typeText 后有真实字节记录。不能将这些工具问题判为 App 卡顿，也不能忽略它们宣布 UI 全过。

- 60 节点 ZIP 已生成并通过媒体解码、哈希和 CRC 校验，但文件选择器未可靠完成导入，已取消；最终列表与数据库没有多媒体新画布。样本是重复 320×180 素材，本来也不代表真实 4K 压力。
- 多媒体下平移、缩放、拖节点、视频播放、同时输出终端，均未完成原生性能验收。
- 空测试画布一次面板宽度改变可见生效，高输出中拖动却受坐标错误阻断；最终 PTY 行列/通知频率/拖动流畅度未验证。
- 未取得可信 FPS、p95/p99 帧时间、物理键盘/触控板到画面延迟；未重启主 App、未测安装回滚、复杂前台进程退出及全部异步竞态。

## 自动测试

【事实】Go service/handler 通过（缓存，repository/router 无测试）；后台 Node 改名 1 项通过；Rust PTY smoke 1 项、Rust canvas 协议/锁定/字段约束 3 项通过。PTY smoke 仅证明读到回显，不能当完整性能验收。

【事实】全量 TypeScript 仍为此前记录的 4 文件 8 错误，四文件本轮无 Git diff：canvas-resource-references（4）、video-settings-panel（2）、gemini 正则目标（1）、canvas-agent 消息类型（1）。未跨范围修复，不能称全量类型检查通过。

## 数据终验、日志与保留项目

正式数据库：`/Users/chenhuajin/Library/Application Support/com.chenyuxiaojin.infinitecanvas/infinite-canvas.db`。

独占证据目录：`/Users/chenhuajin/项目/自己的应用/infinite-canvas-backups/local-installs/data-before-independent-test-GqDSxK/`。

【事实】pre-test.db / post-test.db 完整性均 ok，schema 双向零差异。原 9 行画布所有 6 列（含完整 JSON 和更新时间）完全相同，其余 18 表全列双向零差异。仅新增 ID `YD3Si4Ubw0nXbcEjvVq38`、标题「独立验收-终端改名-临时」、0 节点/0 连线。

原提示词 1645、物理目录 1584、类目 12、收藏 0、assets 38；页面合并目录仍 1636（1584+52），不是丢数据。未逐 Blob 核验 WebKit/IndexedDB。完整逐行结果为证据目录的 `post-test-diff.json`。

保留供复现：

1. 上述空测试画布，以及 `/Users/chenhuajin/项目/视频制作台/AI编导/独立验收-QA-urRtjA/` 内的测试绑定。
2. 仓库 `docs/progress/` 的本报告、4 份专项报告、测试/采样脚本和 `fixtures/canvas-audit-multimedia-v1.zip`、manifest。
3. 独占证据目录的前后数据库、manifest、逐行差异 JSON、6 个 terminal-probe JSON、4 个 process JSON。每轮绝对文件名与采样口径见 [原生实测报告](terminal-native-audit-evidence.md)。
4. `/tmp/canvas-native-qa-mC5wgY/`：为绕开中文路径输入创建的 probe/reports/canvas.zip 链接及 fixture.zip 副本，不是 App 备份。

没有删除任何材料或清空废纸篓。App 最终停在“我的画布”，深色主题已恢复，测试 Shell 均关闭。下一步等待用户授权修复，优先图片引用、明确目录 MCP 路径和输出背压，再按相同有限负载及真实媒体重新验收。
