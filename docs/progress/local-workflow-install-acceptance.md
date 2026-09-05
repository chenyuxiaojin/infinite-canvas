# 本地工作流收敛：正式安装与验收

2026-09-04，用户回复“允许”后，已将源码构建并替换到唯一正式入口 `/Users/chenhuajin/Applications/小陈的画布.app`，正常退出、重启后停留首页。名称、版本 `1.0.0`、应用身份 `com.chenyuxiaojin.infinitecanvas` 与数据目录不变。

## 构建与安装

- 在当前仓库 `desktop` 执行 `build:app`，Next、Agent CLI、Go、Tauri 均构建完成。Next 沿用项目配置跳过全量类型检查；这不代表原先已记录的 TypeScript 问题已经解决。
- 首次安装在注销废纸篓新路径时收到 `failed to scan ...: -10814`，安装器回滚；正式旧包主程序哈希与安装前完全一致。核对系统登记确认该准确路径没有记录后，增加仅对此错误、且准确路径确实无登记时允许继续的处理。
- 第二次安装读系统登记时触发默认缓冲上限 `ENOBUFS` 并回滚。实际登记清单为 23,672,473 bytes，改为显式 64 MiB 上限后读取通过。其他注销错误或准确路径仍存在时继续报错，不忽略全部系统错误。
- 最终使用 `install:app` 安装已构建的新包，退出码 0。新增安装器 5 项回归通过；之前本地工作流 11 项和原图 32 项隔离测试均通过。两次真实失败均进入恢复流程；未额外进行破坏性故障注入。
- 正式包严格签名验证通过，主程序 SHA-256 为 `55d4072d895cf7ace589de83cca56134b9f5d39b282b44a763080be6e1422364`。CLI 链接指向正式包，构建目录不再残留第二个 App。
- Spotlight 只返回正式路径；Launch Services 只有正式包可启动，5 条废纸篓历史均标记 `trash launch-disabled`。未重置系统应用数据库、未清空废纸篓。

## 正式 App 实测

- 首页和画布无 App 登录/账号菜单；设置没有账号同步、账号渠道、云存储配置。原有 5 个用户渠道、9 个模型可见，密钥保持遮蔽，未编辑或发起模型调用。
- 生图、视频和工作流正常打开，无登录错误；工作流显示本地模板、新建入口，账号 AI 创建和公开发布入口不见。生图素材选择器只显示“我的素材”，没有服务端素材库页签。
- 案例 4 原画布 `DUkqxVcwRh30uwMAskyxt` 正常打开，48 节点/7 连线；人物和场景原图在原生窗口目视抽验正常，图片操作浮层无云端上传。没有将抽验记为 32 张原图全部视觉验收。
- 普通终端启动后执行只读 `pwd`，输出原片子目录 `/Users/chenhuajin/项目/视频制作台/AI编导/案例4-克兰奇杀妻案`。离开画布、退出 App 后对应主程序/sidecar 已退出，重启后三个服务恢复。
- 提示词中心显示 1,636 个目录条目；搜索“杰克”得到 2 条，查看既有本地提示词全文成功，未主动收藏、未更新订阅。远程全文断网/失败场景和新收藏写入本次未重测。
- 已安装服务的 `/login`、`/admin` 返回 307 到 `/`，`/asset-library` 返回 307 到 `/assets`。
- Next/Go/Bridge 分别仅监听 `127.0.0.1:3100/3101/3102`；未携带凭据读取 Bridge 项目返回 401。正式 CLI 带既有凭据读取成功，三片子实际 MCP 命令的 `initialize`、`tools/list`、`canvas_context` 均成功，节点/连线分别 17/10、5/11、48/7。
- 账号请求停止有真实源码模块的隔离网络断言和旧入口重定向证据；原生页面本次没有登录弹窗或错误。当前壳会消费并丢弃 sidecar 标准输出，本次没有全程网络抓包，不宣称已对原生整个会话的每个请求逐条审计。

## 数据对账

- SQLite 安装前后快照完整性均为 `ok`，schema 不变。18 张非画布表逐行完全相同，10 个画布无增删，所有节点和连线相同；案例 4 只有 `viewport`、`updatedAt` 及行更新时间变化。
- Application Support 内除数据库/历史数据库备份/日志外的 71 个文件全部逐字节哈希一致，无新增或缺失，包括原图和 Bridge 凭据。
- 三个片子绑定和六个 MCP 配置最终与本轮开始时逐字一致。打开案例 4 终端会由既有设置流程去掉 `.mcp.json` 末尾换行；已证明唯一差异并恢复原字节，未改命令、绑定或其他配置。
- WebKit LocalStorage 的 5 条记录全部相同。两套 IndexedDB 表和记录均对账；除案例 4 缓存与画布索引值外，其余按 store/key 比较的值不变。内部 recordID 会因缓存重写变化，不将它当作用户内容变更。本机工作流、素材、生成历史存储无增删，已存在的外部 IndexedDB Blob 文件哈希一致。
- 原账号/服务端独有内容仍留原位：`users` 1 行、`assets` 38 行、服务端 `creative_workflows` 1 行；这些记录没有自动转成“我的素材”，本机素材选择器当前为空。提示词原表 1,645 行、目录表 1,584 行、收藏 0 行均未变。
- 没有修改用户的 AI 工具登录、API 密钥、Git 远端；没有提交、推送、发布、渲染或付费生成。

## 备份与证据

私有目录：`/Users/chenhuajin/项目/自己的应用/infinite-canvas-backups/local-installs/local-workflow-20260904-191248/`。

- `application-support-before.zip`、`webkit-before.zip`：完整备份且 ZIP 检查通过。
- `before.db`、`after-native.db`、`database-comparison.json`：SQLite 快照和逐表对账。
- `files-and-webkit-comparison.json`、`webkit-detail-comparison.json`：原文件、绑定及浏览器存储核对；浏览器数据库快照也在同目录。
- `build-install.log`、`install-recovery.log`、`install-final.log`：构建、回滚和最终安装证据。
- `installed-runtime.json`、`bridge-readback.json`、`mcp-readback.json`、`app-registration-final.json`：正式包身份、路由、服务和绑定读回。

最终安装对应的旧包 ZIP 为同级 `小陈的画布-2026-09-04T11-15-28-442Z-86521.zip`，通过 ZIP 校验；旧包在 `/Users/chenhuajin/.Trash/小陈的画布-替换前-2026-09-04T11-15-28-442Z-86521.app` 可恢复。两次回滚时生成的 ZIP 亦保留。
