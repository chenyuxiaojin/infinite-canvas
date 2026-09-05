# 普通终端独立审查证据

后续真实 App 的 6 轮有限输出、输入、进程采样与会话回收结果见 [原生实测证据](terminal-native-audit-evidence.md)。下文保留源码/隔离测试的原始边界；其中高流量超限故障仍未关闭。

## 范围与结论

【事实】本报告只覆盖当前工作区的终端代码、现有 Rust smoke 测试，以及使用当前已安装依赖执行的隔离内存测试。没有操作 App 界面、正式数据、项目绑定文件、账号或模型。只新增本报告、`terminal-audit.test.mjs` 与交给主验收代理运行的 `native-terminal-probe.mjs`，未改生产代码、未安装、未提交。

【事实】当前代码的重复发送、逐块有损解码、标题变化重开终端、尺寸回调未合并四个原问题都有针对性改动；其中二进制解码和通道重排已经用真实依赖独立测试。普通终端的真实界面、键盘响应、拖动尺寸和 Shell 生命周期仍不能因此宣称通过。

【事实】受控洪峰测试复现一个故障：xterm 待解析队列超过约 50 MB 后抛异常，而 Tauri Channel 因用户回调抛异常不再推进消息序号。后续输出与 EOF 一直排队，即使 xterm 队列已经清空也不恢复。当前产品没有背压或异常恢复，存在这个故障链。

【未知】真实 macOS App 在日常命令、持续大输出、同时操作画布时多快积压到该阈值、实际按键延迟与 CPU/内存峰值，本报告没有实测；受控队列洪峰不是实机性能验收。

## 源码基线

- 工作区：`/Users/chenhuajin/项目/自己的应用/infinite-canvas`，已有大量用户改动，均保留。
- 比较基线：Git HEAD `5d8eea13b415ff86062670a03cbffed4866162e0`。它只是旧源码，不等同于已安装旧 App，也没有旧 App 的响应延迟基线。
- 当前系统：macOS 26.6.2 / 25G83，arm64；测试 Node v24.12.0。
- 当前依赖：`@xterm/xterm` 6.0.0、Tauri Rust 2.11.5、portable-pty 0.8.1；以当前本地依赖源代码为证据。
- 当前生产文件 SHA-256：
  - `desktop/src-tauri/src/terminal.rs`：`e9a3f62451597afe58c567f9a1e214ca5d9e1310b910873d1b8f7b28e12bbb54`
  - `web/src/services/desktop-terminal.ts`：`37aec4230e210f988b1d503e98ddbec151191fbae698f6c2d5290de92e2cb885`
  - `web/src/app/(user)/canvas/components/canvas-terminal-drawer.tsx`：`fd17365803a2391a79c25bab016ff63861cd439688397edd4d9ad311e8e9e15c`

## 已通过的有限项目

| 项目 | 证据 | 结论边界 |
| --- | --- | --- |
| 单一二进制输出 | `terminal.rs:114–130` 只在读取线程调用一个 Channel；`desktop-terminal.ts:29–33` 转为 Uint8Array；组件 `:116–118` 写入 xterm | 静态确认没有旧版双 emit；不是已安装 App 端到端验收 |
| 中文/emoji 跨块 | 真实 xterm 对 35 字节字符串的全部 34 个分割边界均还原正确；旧逐块独立解码模型有 22 个边界损坏 | 是实际 xterm 包的解码测试，不是 Rust→WebKit 全链路 |
| 重复字符、行序、EOF | 真实服务模块 + 真实 Tauri JS Channel；1000 行 / 30000 字节 / 1765 块，逆序注入且 EOF 先到；最终行内容逐行一致，EOF 一次、回调已清理 | 传输入口被内存替代，不调用 App IPC；可以证明 JS 服务与 Channel 的这部分逻辑 |
| resize 合并 | 组件 `:78–95` 使用一个 rAF，比较实际 cols/rows 后才调用 PTY；`:109–115` 用当前尺寸启动 | 代码级通过；未实测拖动时帧率、PTY 通知数量、最后尺寸一致性 |
| 标题/主题不重启 | 组件 `:153` 会话 effect 仅依赖 projectId / restartKey；`:155–157` 单独更新主题，标题通过 ref 读取 | 代码级通过；未在真实 App 中核对同一 Shell PID |
| 迟到 spawn 回收 | 组件 `:105–106` 在工作目录解析后检查取消；`:126–128` 在 spawn 完成后再次检查；`:143–152` 清理、断开观察、释放 xterm 并终止会话 | 代码覆盖了两处 async 间隙；未做真实快速开关/切项目过程表验证 |
| 不自动启动 AI | 组件不再有 Agent 启动函数或品牌按钮；`:179–180` 仅 paste 且不发送 Enter；Rust 只启动登录 Shell | Shell 自身用户配置不在这个静态结论范围内 |

旧源码对应证据：`terminal.rs` HEAD 133 行逐块 `from_utf8_lossy`，138–139 行同时 `emit` 与 `emit_to`，148–149 行退出也双发；组件 HEAD 191 行 effect 依赖 projectTitle，异步初始化没有取消保护。它说明改动针对真实代码问题，但不能推导“新版快了多少”。

## 失败与风险

### F1：没有背压，输出超限可使整个 Channel 停止前进

【事实】生产链路：`terminal.rs:118–124` 不等待前端解析确认，`canvas-terminal-drawer.tsx:116–118` 不用 xterm write 完成回调，也没有捕获异常。

【事实】当前 Tauri Rust 的 `src/ipc/channel.rs:169–180` 把大于等于 1024 字节的 Raw 消息加入 `ChannelDataIpcQueue` 后排队 JS fetch；`:292–296` 的 send 不是 xterm 消费确认。该队列为 HashMap，没有此链路可见的有界字节预算。JS `@tauri-apps/api/core.js:95–104` 先调用用户回调，成功返回后才增加下一个序号。

【事实】当前 xterm `src/common/input/WriteBuffer.ts:20,103–106` 在待解析数据大于 50000000 字节时抛出 `write data discarded, use flow control to avoid losing data`。生产回调未捕获它，抛出时 Tauri 序号就不再前进。

复现：在仓库根目录执行：

```bash
/usr/local/bin/node --test docs/progress/terminal-audit.test.mjs
```

第 4 个测试使用真实 xterm writer 和真实 Channel，连续同步注入 16384 字节块；在第 3052 个零基序号处失败，此时待解析 50003968 字节。然后注入后续数据、EOF 和 Channel end，均不再交付，callback 仍留存。测试再仅为快速清理把解析动作设为 no-op，等队列排空，继续发数据仍不交付。故障发生之前未替换 writer 或 parser。

【事实】测试中的“4 项通过”指证据断言通过，第 4 项是成功复现产品风险，不是产品高流量通过。

【推断】最小修复方向：用 xterm `write(data, callback)` 的消费确认来控制 Rust 继续读取的字节预算，并给关闭/取消释放等待；同时把 write 异常转换为可见故障并安全结束当前通道，防止序号永久卡住。单纯捕获并忽略异常会丢输出，不能视为通过。缓存阈值需要实机测量后选定，不做架构重写。

### R1：Shell 回收尚缺完整生命周期证据

【事实】`terminal.rs:119–130` 的 EOF 只发通知，未从 sessions 移除或主动 wait；自然退出而面板仍开着时，其 session 记录仍保留。`:195–200` 关闭只移除并调用 child.kill；应用 `lib.rs:334–337` 的退出流程只显式停止 Bridge、运行时和 sidecars，没有遍历 TerminalManager。

【事实】不能简单把“没有显式 wait”直接说成所有关闭都会留下僵尸。portable-pty 0.8.1 `src/lib.rs:329–361` 的 kill 实际会先发 SIGHUP，最多约 200 ms 内进行 try_wait；正常退出可能在这里已经回收。超过宽限期则调用系统 kill，该 fallback 后没有等待。它没有显式处理 Shell 整个子进程组。

【未知】正常关闭、输出中关闭、快速切项目、应用退出后，实际 Shell/前台命令是否都消失，以及是否存在僵尸或持有 PTY 的后代。需按 PID/PPID、命令、启动时间和 PTY 文件描述符逐项确认，不能仅数进程名。

### R2：普通终端会隐藏目录绑定失败

【事实】`project_binding.rs:102–117` 只搜索 AI编导 根目录的直接子目录；`:149–165` 无匹配时返回根目录、configured=false。组件 `:105–115` 只取 projectDirectory，未显示 configured/configurationError；旧版的绑定状态提示已随简化删除。

【推断】用户可能看到可用 Shell 却以为已连接当前画布。是否需要保留一个简短的“未连接画布”提示可另行确认；不需要恢复 AI 品牌按钮。

测试必须先确认隔离片子确实绑定且 cwd 正确。打开已匹配目录时 `project_binding.rs:131–133` 会调用 setup_project_binding，需事先备份对应配置文件。

## 执行记录与性能边界

### 现有 Rust smoke

```bash
cd desktop/src-tauri
PATH=/Users/chenhuajin/.bun/bin:/Users/chenhuajin/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin /usr/bin/time -l /Users/chenhuajin/.cargo/bin/cargo test --locked --offline terminal::tests::test_pty_spawn -- --exact --nocapture
```

【事实】1 测试通过，测试执行 0.07 秒；含编译总墙钟 11.33 秒、user 10.60 秒、sys 2.16 秒、最大 RSS 895369216 字节。该内存是整个测试/编译命令的数据，不是 App 运行内存。

【事实】输出仅抓到 `echo HELLO_PTY\r\n`，即输入命令的 PTY 回显。现有测试 `terminal.rs:224–230` 只断言第一次 read 的 n > 0，未等待实际执行结果，未调用生产 pty_spawn/Channel，因此覆盖很弱。

### 新增隔离测试

【事实】一次执行结果：4 项证据测试通过、总墙钟 0.34 秒、user 0.31 秒、sys 0.02 秒、最大 RSS 177717248 字节。

其中 8400000 字节有限输出解析：68.53 ms 墙钟、99.20 ms user CPU、2.76 ms sys CPU；结束时 Node RSS 177602560 字节；16 ms 心跳仅采到 2 次，最大额外延迟 11.61 ms。

【事实】这组数字属于 Node 中不打开 DOM 的 xterm 解析，不包含 PTY、IPC、WebView 布局/绘制、键盘或画布，也不是 Rust 改动后的 App 加速证明。心跳样本太少，不能据此给 p95/p99 或稳定帧率结论。

## 仍需真实 App 验收

原生探针已提供：`docs/progress/native-terminal-probe.mjs`。在 App 的普通终端中通过正常输入运行，接受 `burst 1`、`burst 8` 或 `stream 15 512`，以及必填的 `--report-dir /明确已有备份目录`。程序只独占新建 JSON 报告，不修改 cwd 下文件；Ctrl+C 可中止、结束恢复原始输入模式。每次 DSR 限时 5 秒，超时后停止发新 DSR，防止无请求 ID 的迟到响应被误归到下一次测试。提供脚本及语法检查不等于它已完成真实 App 验收，实机结果由主验收报告补充。

1. 已绑定的隔离片子内，普通输出、1000 行编号、逐字节中文/emoji 显示不重不漏；核对实际 cwd 和 PID。
2. 有限持续输出中每秒输入一个标记，记录输入到出现的延迟分布；同步记录主 App/WebContent/Node/Go CPU 与 RSS。先低流量逐步增加，停止条件为数据缺失、连续异常延迟或内存持续上涨，不直接向正式 App 注入上述 50 MB 洪峰。
3. 同输出量下拖宽面板、停手后检查 stty rows/columns 与可见终端一致；改标题/切主题后 PID 和已设置的本地变量不变。
4. 工作目录解析中关闭、spawn 中关闭、连续重启、切项目、正常 exit，各重复有限次数，逐个核对对应 Shell 及子进程是否已结束。
5. 最后再测同一终端输出同时进行画布平移、缩放、拖节点和媒体多的画布交互，分别记录渲染与后台压力。Rust 外壳不承担 React/WebView 的全部界面绘制工作，不能用语言选择代替这项验收。
