# 普通终端消费确认与失败清理（顺序修复第 3 步）

日期：2026-09-04。范围：终端 Rust、前端传输、终端面板、命令登记、隔离测试。本专项未构建或安装 App，未操作正式 UI，未改项目绑定、画布、媒体、账号、安装逻辑或 AI 启动方式；安装与原生验收由主任务执行。

## 最新状态：原生发现吞吐回退后的累计确认修复

【事实】首次安装后，主任务发现 64 MiB 有限突发输出没有挂起或探针报错，但生产耗时 98,434.704792 ms，约 0.65 MiB/s。已读取原始 `terminal-probe-2026-09-04T09-35-44-588Z-69205.json`：payload 67,108,800 bytes，尾部 DSR 2.329209 ms，aborted=false、error=null。目录为 `../infinite-canvas-backups/local-installs/ordered-repair-92iTyc/`（相对仓库根）。这证明生产与尾部解析完成，不是逐字节画面/全行哈希核验；burst 的同步 TTY 写阻塞也会延后探针读取输入，不能把其中 98 秒后才记录的按键误报为真实交互延迟。

【事实】进一步用当前 Rust 核心和真实 macOS PTY 输出 1 MiB、即时 ACK，测到 1,048,682 bytes（含 Shell 提示/回显）共 1,046 包，最小 1、最大 1,024、平均 1,002 bytes，0.02 秒完成。16 KiB 是读缓冲上限，不是实际包大小。首次前端逐包串行 ACK 意味着 1 MiB 约千次、64 MiB 约 6.5 万次等待；真实小包形态和源码串行链共同解释了明显的往返放大，但原生尚未直接计数每一个 ACK，不能断言它是全部开销。

【事实】已在用户授权的第 3 步范围内完成第二次性能修复并再次冻结：

- xterm 消费回调仍是唯一 ACK 来源。同一个解析轮内用微任务合并到最新已消费边界；最多一个 ACK 请求在途，其间只保留一个最新累计数，不再为每个包追加 Promise/RPC 队列。
- Rust 允许确认任一准确的已发送包末边界，并一次移除之前已消费边界；拒绝非边界、超前、倒退确认，重复当前值不增加额度。预算仍为 256 KiB，单包上限仍 16 KiB；未用加大积压、跳过消费回调或丢数据换速度。
- EOF 等最新累计 ACK 请求成功，最后一个 ACK 失败不能显示正常退出；同步 invoke 抛错也进入可见失败。关闭后不发送待合并 ACK；消费暂停时无 ACK、无定时杀 Shell，恢复后累计确认。后台真实窗口恢复仍待原生复测。

真实 xterm/Channel + 实际异步计时器的受控对照：2 MiB 相同字节，1 KiB 小包，每次 ACK 延迟 2 ms。首次串行 service 保存在 `fixtures/terminal-serial-ack.ts`，与当前源码在同一测试环境运行：

| 指标 | 首次逐包串行 ACK | 当前累计 ACK |
| --- | ---: | ---: |
| 确认请求数 | 2,048 | 8 |
| 耗时 | 4,706.35 ms | 54.60 ms |
| 最大未确认字节 | 262,144 | 262,144 |
| xterm 最大 pendingData | 262,144 | 262,144 |
| 最大同时在途 ACK | 1 | 1 |
| 送达并确认字节 | 2,097,152 | 2,097,152 |

两版送达 SHA-256 均为 `9318eab1ee4b7c66db0ed05b368b6124635354ce7ddd35d3954d0f94b86ce90c`，EOF 一次、回调无遗留。较前一轮复测也得到 2,048 → 8 次、4,789.13 → 54.35 ms。测试断言请求数下降及边界/完整性，不用易波动的墙钟比值作为唯一通过条件。此对照证实串行小包 ACK 的成本，不等于原生能加速 86 倍。

【事实】最新最终回归：前端 **60/60**（当前终端 24、历史风险 4、图片 32），Rust **37 通过、1 opt-in 跳过**（终端 14 项）。56 MiB 大输出依然逐字节哈希一致、262,144 bytes 上限、EOF 一次；累计边界/重复授信/迟到确认、同步与异步失败、EOF 失败、消费暂停恢复、关闭时有在途确认，以及此前真实 PTY 退出清理均通过。全量 TypeScript 仍为原四文件八处历史错误；`git diff --check` 通过。

【事实】主任务已完成第二次正式安装及同参数原生复测：1 MiB 113.18 ms；64 MiB 7,010.67 ms（首次 98,434.70 ms），尾 DSR 9.006 ms；15 秒 512 KiB/s 持续输出期间 XYZ/回车各接收一次，22 次解析往返全成功。另验输出中拖宽、关闭清理、重开、正常退出及重启；原画布内容和绑定未变。实际原生收益不能套用上面的隔离比值，详情、原始证据和最终二进制哈希见 [最终顺序验收](ordered-repair-acceptance.md)。第二次生产源码只改 `terminal.rs` 的 ACK 边界验证和 `desktop-terminal.ts` 的确认调度，其他新增/调整为测试与文档。

【未知】原生后台暂停/恢复、长期复杂子进程、全部画布帧时间仍未覆盖，消费完成不等于绘制完成。

## 首次源码交付结果（历史，保留诊断过程）

【事实】源码修复和隔离回归通过，可交由主任务统一构建安装。前端 55 项通过，其中新终端协议 19 项、历史故障复现 4 项、图片回归 32 项；Rust 35 项通过、1 项需明确真实图片路径的 opt-in 测试跳过。此次 Rust 包含终端 12 项，使用真正 PTY 的启动、关闭、前台子进程回收及退出清理核心测试通过。

【未知】新源码尚未进入正式 App，不能用 Node 解析耗时或 Rust 单测宣称原生窗口流畅。高输出时输入、拖面板宽度、后台暂停/恢复、实际 WebKit 内存、退出 App 等仍待统一安装后验收。

## 已定位原因与修复

- 旧输出只有传输，没有消费确认；xterm 约 50 MB 队列超限会抛 `write data discarded`，异常逃进有序 Channel 后使后续数据及 EOF 卡住。旧复现保留在 `terminal-audit.test.mjs`，其 service 样本已固定为修复前逻辑，不把历史风险测试冒充当前实现测试。
- 每会话未消费预算为 256 KiB，单包最多 16 KiB。Rust 在发包前登记累计字节和包边界；预算用完只等待该会话，不持全局 manager 锁，不继续读取 PTY。操作系统 PTY 的有限缓冲继续向生产者施加背压。
- 前端把原始字节交给 `term.write(data, consumed)`；只在 xterm 消费回调中提交累计 ACK。第一轮按包串行、只允许下一个边界，已被上面的累计确认修复替代；当前允许准确已发送边界的累计消费，不接受倒退、非边界或超前确认。重复消费回调不重复授信。
- EOF 必须等所有字节消费完成；前端还等最后一个 ACK 请求成功后才显示正常退出。输出异常、同步 xterm 写异常、无效包、无效消费顺序或 ACK 拒绝走可见失败，停止后续 ACK、解除 Channel 回调并请求关闭会话，不假装正常 EOF。
- 先登记会话再启动 Shell/发送首包；启动失败、启动中取消和迟到启动均清理原会话。Rust 按 Arc 身份删除旧槽位，不让迟到清理删除同 ID 的新会话；前端在旧 spawn 尚未返回时继续保留 ID，避免补偿 terminate 命中新 spawn。
- 关闭先唤醒预算/EOF 等待，再停止该 PTY 的准确前台进程组及 Shell，关闭 PTY master/writer，最后 wait 回收。前台组必须属于该 Shell 的 OS session，不按任意 PID 杀进程。App 退出调用同一个 manager shutdown 核心。
- 删除了草稿中的固定 30 秒未 ACK 自动杀 Shell 策略：后台 WebView 暂停本身不视为失败，等待有界但不限时，消费恢复后继续。明确关闭、ACK 拒绝、读取/传输失败会释放等待；这不代表能够自动识别所有静默丢回调/运行时失联场景。

256 KiB 低于 xterm 官方建议的 500 KB 高水位；消费回调表示解析完成，不等于屏幕绘制完成。依据：[xterm Flow Control](https://xtermjs.org/docs/guides/flowcontrol/)。

当前安装的 Tauri 2 Channel 类型没有公开 dispose 方法；本地已检查其实际实现，使用相同的 `window.__TAURI_INTERNALS__.unregisterCallback` 注销回调，兼顾 Rust 尚未得到 Channel 就启动失败的路径。这是版本相关内部接口，升级 Tauri 时须跑回归，不应声称为稳定公开 API。

## 真实取消测试发现并关闭的 macOS 清理问题

【事实】新增真 PTY 取消测试曾卡住：测试进程 67091 的 Shell 67102 为 `?Es`，采样堆栈是 `TerminalSession::stop → Child::wait → __wait4`，输出线程已退出。`lsof` 只剩两个 `/dev/ptmx` 句柄，分别被 session 的 master/writer 保留。该测试明确继续持有 session Arc，因此先 wait、后等 Arc 自然释放不能完成。

【事实】底层 portable-pty 0.8.1 的 Child::kill 已经有 SIGHUP、200 ms 后 SIGKILL，并非遗漏强杀信号。改成先 drop master/writer 再 wait 后，原空闲/满预算取消测试在 0.28 秒内通过。启动取消也改为先让 reader 离开作用域，再由准确的原 session 关闭其余句柄并 wait。

【事实】为结束旧代码造成的卡住测试，仅定向 TERM 测试 PID 67091；之后 67039（cargo）、67091、67102 均不存在。正式 App 51673 未动。原采样保留在 `/tmp/infinite_canvas_desktop_lib-f9c18be05fd06502_2026-09-04_172611_sIp2.sample.txt`，临时目录文件不保证长期保存。

最终真 PTY 取消用例覆盖空闲、256 KiB 满预算、manager shutdown、无效 ACK 四条路径；均等待 reader 与 Channel 被销毁，检查 manager 空、child/master/writer 空、Shell PID 与所测前台组 PID 消失。不同于所有后台/脱离会话的复杂进程树保证。

## 测试结果与复现命令

```sh
/usr/local/bin/node --test docs/progress/terminal-audit.test.mjs docs/progress/terminal-backpressure.test.mjs docs/progress/canvas-local-image.test.mjs
/Users/chenhuajin/.cargo/bin/cargo test --offline --manifest-path desktop/src-tauri/Cargo.toml --lib
cd web
./node_modules/.bin/tsc --noEmit --incremental false
```

【事实】前端真实当前 service 经 TypeScript 转译，使用实际安装的 xterm 与 Tauri Channel，只有 IPC 对端为确定性替身。大输出测试不是原生 Rust/WebKit 端到端测试：

| 指标 | 实际结果 |
| --- | ---: |
| 输入/送达/确认总字节 | 58,720,286 |
| 最大未确认字节 | 262,144 |
| xterm 最大 pendingData | 262,144 |
| 正常 EOF | 1 |
| 尾部中文/emoji 标记 | 恰好 1 次 |
| 遗留 Channel 回调 | 0 |

完整字节 SHA-256 为 `0d83aea837dff7df3bbcd9c07332b48a4deba2162a719561b4680c63b32d1c7b`，发送与送达一致。其余新测试覆盖所有中文/emoji 分块点、Channel 乱序恢复、消费延迟、ACK 请求延迟、先授信后请求返回的合法竞态、同步写异常、拒绝 ACK、消费中关闭、迟到 spawn、启动失败和无效输出。Node 大输出解析约 0.71 秒，不是 App 帧率或 UI 验收指标。

Rust 最终结果：`35 passed; 0 failed; 1 ignored`，0.78 秒。终端 12 项包括预算/边界校验、重复 ACK、慢消费者不占全局锁、关闭与错误唤醒、EOF 排空顺序、逐字节一致、读取/传输错误、旧会话身份保护、失败启动、启动取消、真实 Shell 中文输出与真实取消/退出回收。图片 opt-in 已在第 1 步单独完成，并非此次默认命令执行。

全量 TypeScript 仍为原先四个未改动文件八处错误：`canvas-resource-references.ts` 4 处、`video-settings-panel.tsx` 2 处、`gemini.ts` 1 处、`canvas-agent.ts` 1 处；终端改动未新增类型错误。源码范围 `git diff --check` 通过。未跨范围修复这些历史问题。

## 源码与后续验收

终端源码：`desktop/src-tauri/src/terminal.rs`、`desktop/src-tauri/src/lib.rs`（命令及退出挂接）、`desktop/src-tauri/permissions/desktop-runtime.toml`（ACK 权限）、`web/src/services/desktop-terminal.ts`、`web/src/app/(user)/canvas/components/canvas-terminal-drawer.tsx`。

测试：`docs/progress/terminal-backpressure.test.mjs`；历史对照：`docs/progress/terminal-audit.test.mjs`。共享文件保留图片及此前用户改动，没有提交、打标签、运行模型、生成媒体或安装包。

统一安装后请在明确隔离片子中运行有限输出，核对尾标记/解析往返，观察高输出中的输入及宽度拖动；检查后台恢复、关闭/重启终端和退出 App 后准确 Shell/前台进程消失；最后按主任务已有基线核对数据库、原项目绑定、图片与应用唯一登记。不以“测试通过”代替这些原生验收。
