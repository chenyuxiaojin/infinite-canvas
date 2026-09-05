# 项目旧路径与应用登记修复（第 2 步）

## 已完成

- 按用户授权顺序，在图片源码专项测试通过后修复本步骤。使用 macOS automation audit 的先核对链路、后最小修改、再读回原则；没有修改 Agent 全局配置、账号、凭据、原图或片子内容。
- 案例 2/3/4 的 `.mcp.json` 和 `.codex/config.toml` 共 6 个命令由旧 `~/Applications/无限画布.app/Contents/MacOS/infinite-canvas` 更新为 `~/Applications/小陈的画布.app/Contents/MacOS/infinite-canvas`。与备份比较，差异仅这一命令路径和文件末尾换行；其他配置保留。
- 三个 `.infinite-canvas/project.json` 与备份逐字相同，未改画布 ID、标题或目录。
- `project_binding.rs` 的标题候选不再包含已有绑定文件的目录，损坏绑定也不视为未绑定。精确 ID 查找优先保持原行为，用户主动选择目录的绑定入口不变。避免同名案例 4 把已经绑定的 48 节点画布自动换成另一个 ID。
- 定向注销两个已经在废纸篓中的旧包登记，未移动、删除旧包，未清空废纸篓，未重置系统应用数据库。读回 Launch Services 仅剩正式 `~/Applications/小陈的画布.app`；Spotlight 也仅返回这一正式路径。

## 备份与恢复

本次私有备份目录：`/Users/chenhuajin/项目/自己的应用/infinite-canvas-backups/local-installs/ordered-repair-92iTyc/`。

按片子原目录结构备份了上述 6 份连接配置和 3 份绑定文件，复制后逐字核对一致。`before-repair.db` 为从只读数据库连接生成的在线快照，完整性检查为 `ok`，含 10 个画布记录。恢复配置时可从对应备份文件复制回来；两个旧 App 仍在原废纸篓路径，系统登记如需回滚可针对该路径重新登记。

本次注销的准确路径：

- `/Users/chenhuajin/.Trash/无限画布-替换前-2026-09-04T04-36-36-610Z-10842.app`
- `/Users/chenhuajin/.Trash/小陈的画布-替换前-2026-09-04T07-45-56-019Z-51194.app`

注销前核验均为非软链接 App 目录，identifier 均为 `com.chenyuxiaojin.infinitecanvas`；正式 App 及数据身份未改。

## 实测

从每个片子的实际 MCP 配置命令，在该片子工作目录执行只读 `initialize`、`tools/list`、`canvas_context`，没有启动任何 AI 模型。

| 片子 | 原画布 ID | 节点 / 连线 | 结果 |
| --- | --- | --- | --- |
| 案例2-美甲师日常 | `case2-mjs-ep01` | 17 / 10 | 四个 MCP 工具可发现，读取成功 |
| 案例3-国运末世 | `case3-guoyun-ep01` | 5 / 11 | 四个 MCP 工具可发现，读取成功 |
| 案例4-克兰奇杀妻案 | `DUkqxVcwRh30uwMAskyxt` | 48 / 7 | 四个 MCP 工具可发现，读取成功 |

Rust 项目目录专项测试 3 项通过：未绑定目录原有匹配、同标题异 ID 不覆盖、损坏绑定不覆盖。正常 ID 查找和绑定文件不变亦包含在断言中。

第二步结束后再次从只读源生成 `after-step2.db`，使用 `verify-repair-data.mjs` 逐表全字段排序哈希核对：19 张表、原 10 个画布、所有提示词/收藏/素材/账号记录全部相同，schema 未变，两个快照完整性均为 `ok`。证据保存在同一备份目录的 `step2-data-comparison.json`，没有将敏感表内容写入报告。

## 最终统一安装后读回

- 源码中的绑定保护已随本轮正式 1.0.0 安装。六处连接路径最终正确，三个绑定与修改前备份逐字相同。已经运行中的 Agent 可能需要用户自行重启该会话才能重读配置，本次没有结束任何用户 Agent。
- 系统再次扫描后，Launch Services 保留 4 条废纸篓旧包历史，全部标记 `trash launch-disabled`；只有正式 App 可启动。Spotlight 返回唯一正式路径，CUA 应用枚举只有一个画布，无独立 Node。此前“仅剩正式登记”是当时注销后的快照，不能解释为历史记录永不再出现。没有为消除禁用历史记录而永久删除旧包或重置系统数据库。
- 最终数据库与修复前比较：18 张非画布表完全相同；画布只发生案例 4 定位视口/更新时间和 QA 更新时间变化，所有节点/连线、画布总数和 schema 不变，完整性为 `ok`。见 [最终顺序验收](ordered-repair-acceptance.md)。
- 尚未对安装器异常回滚作故障注入，也未宣称全盘应用缓存的每一种 UI 均已验证。
