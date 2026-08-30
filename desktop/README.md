# macOS 桌面壳

此目录将现有 Next.js standalone 服务和 Go API 作为固定 sidecar
装入 Tauri 2。网页源码仍在 `web/` 独立维护和构建；桌面壳不复制业务
前端，也不向网页开放任意 shell。

## 本机构建

```bash
cd desktop
bun install --frozen-lockfile
PATH="${HOME}/.cargo/bin:${PATH}" bun run dev
PATH="${HOME}/.cargo/bin:${PATH}" bun run tauri build --bundles app
```

`bun run dev` 会先完整执行资源暂存，再启动 Tauri 开发模式；
`beforeBuildCommand` 在生产打包前调用同一个 `bun run stage`。暂存
过程依次构建 `web`、`infinite-canvas` Agent CLI、arm64 Go sidecar，
从当前 Node 24 运行时提取 arm64 sidecar，并把 Next standalone 资源
放入忽略目录。当前 P1 只验收 Apple Silicon macOS；未来增加其他架构时，
必须分别生成带目标三元组后缀的 sidecar。

运行时固定拓扑：

- Tauri WebView：`http://127.0.0.1:3100`
- Next standalone：只监听 `127.0.0.1:3100`
- Go API：只监听 `127.0.0.1:3101`
- Agent Bridge：只监听 `127.0.0.1:3102`，使用应用数据目录内的安装专属凭据
- SQLite 与后端日志：Tauri 应用数据目录

端口被占用时桌面壳直接报错退出，不连接未知进程。前端 capability
不包含 shell 权限；sidecar 的可执行文件、参数和工作目录全部由 Rust
固定。Agent Bridge 不开放 shell、自由路径或公网监听，详细命令和
总装边界见 [本机 Agent 适配层](../docs/development/local-agent-adapter.md)。App 资源
保留上游 MIT 原作者声明，并包含所打包 Node.js v24.12.0 的许可证。
