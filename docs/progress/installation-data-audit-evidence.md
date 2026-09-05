# 安装、数据与项目绑定独立核验

## 范围与结论

- 核验对象：正式 `/Users/chenhuajin/Applications/小陈的画布.app`，以及固定应用身份下的 SQLite 数据和已绑定片子目录。
- 本轮没有安装、重启、结束进程、更新系统应用登记、改正式数据库、改 WebKit 或重写片子配置。使用 macOS automation audit 的只读链路核验约束；仅生成获准的测试快照和本报告。
- 【事实】正式 App 为 1.0.0；本机严格签名校验、CLI、3100–3102 服务与数据完整性通过。
- 【事实】正式路径/Spotlight 仅一个入口，但 Launch Services 当前仍有两个废纸篓旧包登记；不能称系统登记完全唯一。
- 【事实】案例 2、3、4 的 Claude/Codex 项目 MCP 六处命令仍指向已不存在的旧名称 App；全局 CLI 已更新不等于项目配置已更新。
- 【未知】本轮没有重新安装，因此不将历史快照比较称为本次亲自执行的安装前后验收；没有故障注入验证安装回滚，也没有在此子任务做 Dock/UI 或性能验收。

## 当前安装和运行证据

在 2026-09-04 本次独立测试前读取：

| 检查 | 当前结果 |
| --- | --- |
| `CFBundleName` / `CFBundleDisplayName` | 小陈的画布 |
| `CFBundleShortVersionString` / `CFBundleVersion` | 1.0.0 / 1.0.0 |
| identifier | `com.chenyuxiaojin.infinitecanvas`，未变 |
| `codesign --verify --deep --strict` | 退出码 0 |
| 签名类型 | ad-hoc，TeamIdentifier 未设置；不是发行签名或公证验收 |
| 主程序 | PID 51673，正式 App 内 `infinite-canvas-desktop` |
| Go API | PID 51678，父进程 51673，正式 App 内 `infinite-canvas-api`，`127.0.0.1:3101` |
| Node | PID 51679，父进程 51673，正式 App 内 `node`，`127.0.0.1:3100` |
| Agent Bridge | 主程序 PID 51673，`127.0.0.1:3102` |
| 页面 | GET `http://127.0.0.1:3100/canvas` = HTTP 200 |
| Go 健康接口 | GET `http://127.0.0.1:3101/api/health` = HTTP 200，正文 `ok` |
| Bridge 安全边界 | 无凭据 GET `/v1/capabilities` = HTTP 401；正式 CLI `capabilities` 使用本机凭据成功，退出码 0 |
| 正式 CLI 链接 | `~/.local/bin/infinite-canvas` → 新 App 的 `Contents/MacOS/infinite-canvas` |
| 数据实际打开路径 | Go 进程 `lsof` 指向 `~/Library/Application Support/com.chenyuxiaojin.infinitecanvas/infinite-canvas.db` |

正式二进制 SHA-256：

```text
infinite-canvas-desktop 641c116d410ae32dc9c607c21a7afc4216542e17f8a917e722840a00048c21aa
infinite-canvas-api     8ad6576f7971d4248651be64099b519d044d3682928d04e1598d2812a9103fad
infinite-canvas         baf329a64dd12db818438392f4bbab0cb4224c248d9da8879f00809a635d1480
```

打包资源 `Contents/Resources/web/background-node.cjs` 与源码 SHA-256 均为 `7a3bc580f255bf3ea1910c960fb8ea8fc5357e75a0e30993962d62b39c10fdd8`。本机 Bridge 凭据文件权限仍为 `0600`，本报告没有读取或展示凭据内容。

有限范围检查了用户/系统 Applications 与本仓库 release/bundle，未发现第二个非废纸篓画布 App；`mdfind` 按 identifier 仅返回正式新路径。未遍历无关全盘。

`lsregister -dump` 当前却有以下三个同 identifier 记录：

1. `/Users/chenhuajin/Applications/小陈的画布.app`（1.0.0）。
2. `/Users/chenhuajin/.Trash/无限画布-替换前-2026-09-04T04-36-36-610Z-10842.app`（物理 plist 0.5.5）。
3. `/Users/chenhuajin/.Trash/小陈的画布-替换前-2026-09-04T07-45-56-019Z-51194.app`（物理 plist 0.5.5）。

【事实】静态登记未找到 `name` / `displayName` 为 `node` 或 `next-server` 的记录。主代理另以 CUA `cua.listApps` 原生系统应用枚举，按 `画布|node|next-server` 筛选后只返回正在运行的「小陈的画布」和固定 identifier，没有 Node/next-server 条目。这是原生应用枚举，不是 Dock 截图或应用切换器截图。未尝试清理旧登记或废纸篓。

## 独立测试前基线

UI 压力测试开始前，获主代理明确授权后，用 SQLite 在线 `.backup` 从只读源生成：

`/Users/chenhuajin/项目/自己的应用/infinite-canvas-backups/local-installs/data-before-independent-test-GqDSxK/pre-test.db`

快照完成时间：`2026-09-04T08:32:44.615Z`（UTC）。文件大小 56,647,680 字节，快照 `PRAGMA integrity_check` = `ok`。同目录 `pre-test-manifest.json` 记录 19 张表的行数和完整行内容 SHA-256；哈希方式为每行 JSON 排序后以换行连接，不输出敏感行内容。目录是 `mkdtemp` 创建的私有目录。

| 表 | 快照行数 |
| --- | ---: |
| canvas_projects | 9 |
| prompts | 1645 |
| prompt_catalogs | 1584 |
| prompt_categories | 12 |
| prompt_favorites | 0 |
| assets | 38 |
| creative_workflows | 1 |
| settings | 2 |
| users | 1 |
| agent_operation_requests / ai_call_logs / canvas_audio_tasks / canvas_image_tasks / credit_logs / image_generation_logs / storage_objects / user_configs / video_generation_logs / video_tasks | 各 0 |

【事实】当前只读提示词列表 API 返回 `total=1636`，收藏列表返回 `total=0`。快照按正式查询条件统计为 1584 条启用目录 + 52 条本地提示词 = 1636；展示数不是 `prompt_catalogs` 物理表行数，不能误报目录丢了 52 条。对应组合查询见 `repository/prompt_catalog.go:11`。

## 旧安装前快照与本轮测试前快照比较

旧快照：`../infinite-canvas-backups/local-installs/data-before-outline-20260904-154501/infinite-canvas.db`。

- 【事实】旧快照、本轮测试前快照和当时正式库的 `integrity_check` 均为 `ok`。
- 【事实】逐表执行双向 `EXCEPT` 比较全部列：19 张表中，除 `canvas_projects` 外其余 18 张表均为 `removed=0, added=0`，包括提示词、目录、类目、收藏、素材和账号相关表。
- 【事实】`canvas_projects` 为 `removed=1, added=1`，是同一 ID `DUkqxVcwRh30uwMAskyxt` 行变化。逐键展开 `project_data` 后，仅 `updatedAt` 变化，SQL 行的 `updated_at` 同步变化；所有节点与连线及其余 JSON 字段完全相同。其他 8 行画布完全相同，含 5 行空项目记录。
- 【事实】旧目录的 `database-after-install.json` 记载安装后 19 表双向差异为 0；`database-after-launch.json` 记载打开案例 4 后同样只有其更新时间变化。它们是已有历史记录，不是此次独立安装操作产生。
- 【事实】重新验证旧目录 `app-data.zip`、`webkit.zip` 及同级 `小陈的画布-2026-09-04T07-45-56-019Z-51194.zip`，`unzip -tq` 均无错误。没有解压或改动正式 WebKit。
- 【事实】旧 `source-manifest.json` 的 247 个源文件哈希与此次检查时仓库相同；这可确认历史构建来源清单未漂移，但不能代替性能验收。

## 片子绑定现状

只检查 `AI编导` 的直接片子目录及其三个绑定文件，不读取或修改其他 Agent 全局配置。

| 片子目录 | project.json 的画布 ID | Claude `.mcp.json` | Codex `.codex/config.toml` |
| --- | --- | --- | --- |
| 案例2-美甲师日常 | `case2-mjs-ep01` | 旧路径，不存在 | 旧路径，不存在，enabled=true |
| 案例3-国运末世 | `case3-guoyun-ep01` | 旧路径，不存在 | 旧路径，不存在，enabled=true |
| 案例4-克兰奇杀妻案 | `DUkqxVcwRh30uwMAskyxt` | 旧路径，不存在 | 旧路径，不存在，enabled=true |

以上六处旧路径均为：`/Users/chenhuajin/Applications/无限画布.app/Contents/MacOS/infinite-canvas`。

【事实】`desktop/src-tauri/src/project_binding.rs:120` 的 `configure_workspace` 调用 `setup_project_binding`，并以当前 App 的 bundled CLI 路径更新目录；`resolve_canvas_project_workspace` 会触发该逻辑。静态代码支持“打开终端时更新”，但上述真片子尚未在本轮做写入验收。若要修复，应先分别备份明确目录中的三个配置文件，再使用既有绑定流程，最后回读新路径和原画布 ID；不应批量改其他 Agent 全局配置。

### 既有同名画布绑定风险（未触发）

- 【事实】测试前库有两个未删除的同名案例 4：`case4-clench-murder`（32 节点）与 `DUkqxVcwRh30uwMAskyxt`（48 节点）；真实片子目录当前明确绑定后者。
- 【事实】`project_binding.rs:102–117` 在找不到精确画布 ID 绑定时按标题给目录评分，随后 `configure_workspace` 直接调用写绑定流程。上述文件及 adapter 绑定文件当前均无 Git diff，不是本轮终端改动。
- 【推断】若在旧的 32 节点案例 4 打开终端，按此控制流会选中同名片子目录并把它改绑旧 ID。没有在真实目录触发此场景；应另行处理“标题猜测覆盖已有其他 ID 绑定”的风险，而不是为测试而改真实片子。

## 安装器只读审查

`desktop/scripts/install-local.mjs` 已审读完整：

- 25–31：拒绝 symlink/非目录 App，并核对固定 identifier。
- 42–46、72、89：检查新/旧/构建路径下的运行进程；不 kill。
- 64–71：新旧 App 同时存在则停；仅更新不存在或原来确实指向本 App 的 CLI 链接。
- 74–85：先本机签名和深度严格校验，再压缩旧包并检验 ZIP。
- 95–110：旧包移废纸篓、构建包移正式路径，定向更新登记和 CLI；没有清库、清 WebKit 或全系统登记重置。
- 112–121：存在恢复旧 App/CLI 的异常分支。

【未知】未运行安装器或故障注入，不能宣称所有异常分支都能恢复。当前两个废纸篓登记残留也说明“执行过定向注销”不能单独证明系统登记最终唯一。

## 本轮测试后核对

主代理结束 UI 测试后，于 `2026-09-04T08:48:35.585Z` 再次在线备份为同目录 `post-test.db`，并保存全部差异到同目录 `post-test-diff.json`。

- 【事实】后快照完整性 `ok`，数据库 schema 双向差异 0。19 表全列双向比较：18 表均 0 差异；`canvas_projects` 从 9 行到 10 行，`removed=0, added=1`。
- 【事实】对全部原有 9 个画布逐主键再比较 `user_id`、`id`、`project_data`、`created_at`、`updated_at`、`deleted_at` 六列，全部逐字相等。不只是节点/连线相同，更新时间、视口和面板等嵌套数据也未改变。
- 【事实】唯一新增为 `YD3Si4Ubw0nXbcEjvVq38`「独立验收-终端改名-临时」，0 节点、0 连线；没有新增多媒体画布。原 1645 提示词、1584 目录表记录、12 类目、0 收藏、38 素材及全部其他原记录相同。
- 【事实】案例 2、3、4 的 3 个 `project.json` 与 6 个 Claude/Codex MCP 配置，测试后 SHA-256 均与测试前完全相同；本轮没有把任何真片子改绑。原 MCP 旧路径失效问题仍保留，没有擅自修复。
- 【事实】新临时目录 `/Users/chenhuajin/项目/视频制作台/AI编导/独立验收-QA-urRtjA` 明确绑定新增画布 ID，两份 MCP 命令均为新 App 路径且可执行文件存在。测试画布和临时目录均保留，没有删除用户数据。
- 【事实】测试结束主程序、Go、Node PID 仍为 51673、51678、51679，路径不变；此轮没有更新 App，不称为安装前后验收。
- 【未知】没有对 WebKit / IndexedDB 内的媒体 blob 逐项比较，SQLite `assets` 表相同不能代替浏览器本地媒体逐项验收。

前/后快照完整文件 SHA-256：

```text
pre-test.db  46266524c3ff867d0e9d4411c0cdd73965371d41e1206cd4db025309791bf49a
post-test.db bcc8de49f08e33041a4f020289deb51106428586c149be1bad8886a50cfae8da
```
