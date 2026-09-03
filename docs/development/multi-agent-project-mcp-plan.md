# 无限画布多 Agent 项目级接入方案

> 状态：第一轮已于 2026-09-04 实施并完成本机验收。
>
> 2026-09-02 修订（小陈拍板）：加入目录原则（一个片子目录 = 一张画布，Agent 在片子目录启动）；第一轮只接 Claude Code + Codex，Grok / Gemini Antigravity 放第二轮；网页版下线拆到独立文档 `web-product-sunset-plan.md`；补入 Claude Code 项目级 MCP 的目录发现规则与 `CLAUDE_PROJECT_DIR` 事实。

> 2026-09-04 实施结果：桌面 App 可自动匹配或手选片子目录，生成项目绑定并合并 Claude MCP；右侧可在同一节点上下文中切换画布 Agent 和本地终端；安装包内 CLI 已提供 `mcp serve`、四个极简工具和安全 dry-run。Codex 当前版本不会自动读取片子目录内的 `.codex/config.toml`，所以 App 的 Codex 启动按钮会显式注入这一个 MCP 配置。

## 一、结论

这套方案要解决的核心目标：**人和 Agent 同时编辑同一张画布，互不覆盖**。小陈在桌面 App 里手动改，Agent 通过 MCP 改，像现在人机共同写一份文档一样；换一个 Agent 也能接着上一个 Agent 的结果继续，且上下文消耗控制在片子目录范围内。

无限画布应采用"桌面 App + 项目级极简 MCP + CLI 备用通道"的结构。

- 无限画布 App 负责真正的数据、画布界面、本地媒体与任务执行。
- MCP 服务随 App 安装，全电脑只保留一份，不复制到每个项目。
- 每个片子目录只保存一份轻量绑定信息和各 Agent 的项目级启用配置（目录原则见 4.0）。
- 第一轮接 Claude Code 与 Codex，第二轮再接 Grok 与 Gemini Antigravity；所有 Agent 连接同一个本机 MCP，读写同一个画布工程。
- CLI 继续保留，作为调试、自动化和 MCP 故障时的备用入口，不作为 Agent 日常主要入口。
- 网页版产品形态的取消是独立事项，见 `web-product-sunset-plan.md`；本方案只要求保留桌面 App 内部运行界面所必需的 React/Next.js WebView 代码。

## 二、目标与边界

### 2.1 目标

1. 在同一个片子目录中，第一轮的 Claude Code、Codex 都能自动发现无限画布（第二轮扩展到 Grok、Gemini Antigravity）。
2. 切换 Agent 后不需要重新解释无限画布是什么，也不需要重新粘贴使用说明。
3. 所有接入的 Agent 操作的是同一个画布项目、同一套 revision、任务、锁和审计记录。
4. 未进入相关片子目录时，不加载无限画布 MCP，避免无关上下文消耗。
5. 人工编辑优先，Agent 不得覆盖更新版本，不得绕过付费批准。

### 2.2 不做的事情

- 不做剪辑。画布是编排台（分镜、关键帧、生成任务、素材摆放），切点、时间线、三层声音仍在达芬奇。
- 不在一个片子目录里绑多张画布。系列剧再拍一集，就开新的片子目录。
- 不把完整操作手册塞进 `AGENTS.md` 或 `CLAUDE.md`。
- 不为多个 Agent 分别实现多套画布接口。
- 不建立第二份 Agent 专用画布数据库。
- 不让 MCP 直接操作 SQLite、执行任意 Shell、读取任意本机路径或访问任意 URL。
- 不让 Agent 自动批准付费生成。
- 不把 `/Users/chenhuajin/项目/自己的应用/video-agent-skills/video-agent-producer/SKILL.md` 并入无限画布接入层。它属于视频生产工作流，不是无限画布的基础连接协议。

## 三、现状判断

### 【事实】当前已经具备的基础

- 本机已有正式 CLI：`/Users/chenhuajin/.local/bin/infinite-canvas`，它是 App 包内二进制的软链（`无限画布.app/Contents/MacOS/infinite-canvas`）。
- CLI 默认连接桌面 App 的本机 Agent Bridge：`127.0.0.1:3102`。
- 已有项目读取、画布操作、媒体摄入、任务查询、运行时查询和凭据轮换等结构化能力。
- 既有协议要求 Agent 写入携带 `project_id`、`request_id`、`base_revision` 和 `actor: agent`。
- 既有协作分支使用 `CanvasProject.operationState` 作为 revision、任务、请求记录、锁和审计的唯一事实来源。
- 当前 App 未启动时，CLI 会明确返回 `RUNTIME_UNAVAILABLE`，不会假装执行成功。
- 当前 CLI 已有 `mcp serve`，并提供 `canvas_context`、`canvas_read`、`canvas_mutate`、`canvas_task` 四个工具。
- Codex 官方支持项目级 `.codex/config.toml`，并允许在其中配置 MCP。
- 本机 Claude Code 支持 `local`、`user`、`project` 三种 MCP 范围。
- Claude Code 项目级 `.mcp.json` 只从启动目录（项目根）读取；官方文档没有向上级或子目录查找的表述（2026-09-02 核实，code.claude.com/docs/en/mcp）。
- Claude Code 启动 STDIO MCP 服务时，会在服务进程环境里注入 `CLAUDE_PROJECT_DIR` = 项目根，且不随会话中途增减工作目录而变。
- Claude Code 对 MCP `roots/list` 返回启动目录加所有 `--add-dir` 附加目录，集合变化时发 `notifications/roots/list_changed`。
- 小陈当前在管线层（如 `~/项目/视频制作台/AI编导/`）启动 Claude，达芬奇 MCP 的 `.mcp.json` 也在该层；片子在下一层案例目录（如 `案例2-美甲师日常/`），对应画布工程 `case2-mjs-ep01`。
- 本机 Grok 支持项目级 `.grok/config.toml`；本机 Gemini Antigravity 支持把 MCP 配置、规则和 Skill 打包为项目插件，插件启用时才加载工具（第二轮再核）。

### 【推断】应保留和复用的部分

- 现有 Agent Bridge、CLI 客户端、revision、幂等、人工锁和付费闸门已经形成可靠底座，MCP 应当包装这套能力，而不是重新实现。
- MCP 与 CLI 应共用同一个 Rust 客户端库，避免两套协议逐渐不一致。
- 项目级加载比全局 MCP 更符合当前需求，因为无限画布只与视频、内容和视觉生产项目有关。

### 【已核实】第一轮实施结果与限制

- 本机 Codex 通过 `-C` 进入片子目录时不会自动读取片子目录内的 `.codex/config.toml`；侧栏启动时已通过 `-c` 显式注入无限画布 MCP，项目文件仍保留作后续兼容。
- 当前 Bridge 没有按 revision 返回差异的接口；`canvas_read` 支持按节点 ID 读取并只返回所选节点之间的连线，整图读取需要显式不传节点 ID。
- MCP 服务沿用安装专属的默认 Bridge 凭据文件，绑定文件和各 Agent 配置都不保存凭据。
- Claude Code 能读取片子目录的 `.mcp.json`；首次使用新项目 MCP 时仍会要求用户确认信任。
- 第二轮再核：Grok、Gemini Antigravity 当前安装版本升级后，项目配置格式是否变化；Antigravity 的项目插件是否会被当前工作区自动启用，还是需要首次手动确认。
- 网页版相关未知项已移至 `web-product-sunset-plan.md`。

## 四、推荐架构

```text
片子目录（项目根）
  ├─ 项目级 Agent 规则（CLAUDE.md / AGENTS.md，短）
  ├─ 项目级 MCP 启用配置（.mcp.json / .codex/config.toml）
  └─ .infinite-canvas/project.json（只保存画布绑定）
                │
                ▼
Claude Code / Codex（第一轮） · Grok / Gemini Antigravity（第二轮）
                │
                ▼
STDIO MCP：infinite-canvas mcp serve
                │
                ▼
现有 Rust Agent 客户端 / Agent Bridge 127.0.0.1:3102
                │
                ▼
CanonicalCanvasAdapter → CanvasOperationBatch
                │
                ▼
CanvasProject.operationState（唯一协作状态）
                │
                ▼
无限画布桌面 App

备用通道：Agent 或人工 → infinite-canvas CLI → 同一个 Agent Bridge
```

### 4.0 目录原则（2026-09-02 小陈定）

- **一个片子目录 = 一部片子 = 一张画布。** 在 AI编导 管线里就是一个案例目录，例如 `~/项目/视频制作台/AI编导/案例2-美甲师日常/` 对应画布 `case2-mjs-ep01`。
- **片子目录就是项目根。** `.infinite-canvas/project.json` 和各 Agent 的项目级配置（`.mcp.json`、`.codex/config.toml`）都放在这一层。
- **Agent 必须在片子目录里启动。** Claude Code 的项目级 MCP 只认启动目录，在管线层（`AI编导/`）启动看不到片子目录里的绑定。
- **系列剧再拍一集，就开一个新的片子目录**，不在案例目录内按集分绑定。
- **上一层的规则不会丢。** `CLAUDE.md` 会向上继承，管线层规则在片子目录启动时仍然生效；MCP 配置不会向上继承，所以达芬奇等管线级 MCP 条目要出现在片子目录的 `.mcp.json` 里（由初始化器合并，见第五节）。

### 4.1 为什么 MCP 服务不复制到每个项目

MCP 的"项目级"指项目决定是否启用、绑定哪张画布，不代表每个项目都复制一套服务程序。

推荐方式：

- App 内携带一份 `infinite-canvas` 可执行文件（现状已如此，CLI 就是它）。
- 新增 `infinite-canvas mcp serve`，以 STDIO 方式提供 MCP。
- 每个片子目录的 Agent 配置只引用这个稳定命令。
- App 升级后所有接入的 Agent 自动使用新版服务，不需要逐项目更新代码。

### 4.2 项目绑定文件

每个片子目录新增：

```text
.infinite-canvas/project.json
```

建议只保存：

```json
{
  "schema_version": 1,
  "canvas_project_id": "PROJECT_ID"
}
```

该文件不得保存：

- Agent Bridge 凭据
- API Key
- 绝对媒体路径
- 当前 revision
- 任务运行状态

revision 和任务状态必须实时从无限画布读取，避免多个 Agent 使用过期副本。

服务端定位绑定文件的顺序：

1. 读 `CLAUDE_PROJECT_DIR`（Claude Code 注入），在该目录找 `.infinite-canvas/project.json`。
2. 没有该变量（如 Codex）时，从 MCP 服务进程的当前目录向上逐级查找。
3. 都找不到，返回结构化错误 `NO_PROJECT_BINDING`，提示"请在片子目录启动 Agent，或先运行 `infinite-canvas agents setup`"。不猜测、不回退到任何默认画布。

## 五、Agent 的项目级接入

| 轮次 | Agent | 项目级方式 | 建议 |
| --- | --- | --- | --- |
| 第一轮 | Claude Code | 使用 `claude mcp add --scope project` | 由 Claude 原生命令生成片子目录的 `.mcp.json`，不手写未知格式 |
| 第一轮 | Codex | 片子目录内 `.codex/config.toml` | 配置 `infinite-canvas mcp serve`，只在可信项目加载；开工前核实配置目录发现规则 |
| 第二轮 | Grok | 片子目录内 `.grok/config.toml` | 使用 `grok mcp add --scope project` 生成 |
| 第二轮 | Gemini Antigravity | 项目插件中的 `mcp_config.json` | 插件同时携带极短规则，首次启用后仅随该插件加载 |

各 Agent 的配置格式不同，因此不能假设一份 `.mcp.json` 可以原生通吃。解决办法不是强行统一格式，而是由无限画布提供一次性项目初始化命令：

```text
infinite-canvas agents setup --project 当前片子目录
```

该命令只负责：

1. 识别已安装的 Agent。
2. 展示准备创建或更新的项目配置。
3. 经用户确认后，为已安装 Agent 生成各自的项目级配置。
4. 创建或选择一张无限画布，并写入项目绑定文件。
5. 检查 App 和 MCP 是否可连接。
6. 对片子目录里已有的 `.mcp.json` / `.codex/config.toml` 做合并，不覆盖已有条目。
7. AI编导 管线：把管线层 `.mcp.json` 里的达芬奇条目一并写进片子目录（启动位置下移一层后，管线层配置不再加载）。

后续如配置格式变化，只维护这一个初始化器。

## 六、AGENTS.md 与 CLAUDE.md 的职责

`AGENTS.md`、`CLAUDE.md` 可以让 Agent 知道无限画布，但不能代替 MCP。它们负责"什么时候用"，MCP 负责"实际上怎么操作"。

项目规则只保留类似下面的短说明：

```text
本项目已绑定无限画布。涉及脚本、分镜、图片、视频、音频或视觉编排时，
优先使用项目提供的 infinite-canvas MCP。写入前读取最新 revision 并先 dry-run；
遇到人工锁、版本冲突、付费批准或 App 未启动时停止并向用户说明。
```

推荐：

- `AGENTS.md` 保存这段主规则，供支持目录规则的 Agent 使用。
- `CLAUDE.md` 只保留一行指向同一规则或重复极短版本。
- `CLAUDE.md` 会向上继承，这段规则可以放在片子目录，也可以放在管线层由所有片子共用；MCP 配置不会向上继承，必须在片子目录。
- 不粘贴 CLI 命令大全、JSON Schema 或完整协议。
- 详细说明放在无限画布自己的文档中，Agent 需要时再读取。

## 七、极简 MCP 工具设计

第一版只暴露四个工具：

| 工具 | 用途 | 默认返回内容 |
| --- | --- | --- |
| `canvas_context` | 检查 App、绑定项目、能力和最新 revision | 紧凑状态摘要，含绑定来源文件路径 |
| `canvas_read` | 按节点读取，必要时读取整图 | 需要的节点及所选节点之间的连线 |
| `canvas_mutate` | dry-run 或提交白名单画布操作 | 变更摘要、新 revision、冲突信息 |
| `canvas_task` | 摄入媒体、请求生成、查询或取消任务 | 任务摘要、费用预估、人工批准状态 |

### 工具设计约束

- 不提供 `canvas` 选择参数：一个片子目录只绑一张画布，服务端按 4.2 的顺序自动定位。
- `canvas_context` 返回绑定项目、片子目录、最新 revision、节点与连线数量，方便确认 Agent 在对哪部片子说话。
- `canvas_mutate` 默认 `mode=dry_run`，显式指定后才能提交。
- 服务端自动读取项目绑定，但实际写请求仍必须带最新 `base_revision`。
- 写入必须保留原有 `request_id` 幂等机制。
- `canvas_read` 传入节点 ID 时只返回所选节点和它们之间的连线，避免默认塞入整张大画布。
- `canvas_task` 可以请求付费任务，但只创建 `pending_approval`；MCP 不提供 approve 动作。
- 工具说明要短，公共 MCP instructions 只保留安全边界和标准操作顺序。
- 底层继续沿用白名单操作，不开放任意 JSON、任意文件路径或 Shell。

## 八、上下文消耗方案

| 方式 | 无关项目消耗 | 相关项目消耗 | 无缝程度 | 判断 |
| --- | --- | --- | --- | --- |
| 全局完整 MCP | 高 | 高 | 高 | 不采用 |
| 项目级完整 MCP，暴露大量工具 | 无 | 中到高 | 高 | 不采用 |
| 项目级极简 MCP + 短规则 | 无 | 低到中 | 高 | 推荐 |
| CLI + 短规则 | 无 | 最低 | 中 | 保留为备用 |
| 在规则文件中放完整 CLI 手册 | 无 | 高 | 中 | 不采用 |

上下文控制的关键不是只看 MCP 或 CLI，而是控制三件事：

1. 只在相关片子目录加载。
2. 只暴露少量高层工具。
3. 只按需读取画布局部或 revision 差异。

## 九、网页版产品下线（已拆出）

网页版产品下线与 MCP 接入没有依赖关系，已拆为独立文档：同目录 `web-product-sunset-plan.md`，可单独排期。

本方案只保留一条约束：桌面 App 的界面由 React/Next.js 在 Tauri WebView 中运行，依赖 `127.0.0.1:3100` / `3101`，MCP 接入期间不动 `web/`。

## 十、实施阶段

### 阶段 0：冻结接口与建立基线

- 以现有 CLI、Bridge 和协作协议为基线。
- 确认 `operationState` 仍是唯一协作状态。
- 记录第一轮两个 Agent（Claude Code、Codex）当前版本与项目配置格式；核实 Codex 配置目录发现规则。
- 核实 Bridge 是否已有按 revision 取差异的接口，以及 CLI 默认凭据位置。
- 先备份正式 App 和画布数据库，再做接入开发。

通过标准：CLI 的读取、dry-run、写入、版本冲突、锁和任务状态均可重复验证。

### 阶段 1：只读 MCP

- 在现有 Rust Agent 适配层新增 STDIO MCP 模式。
- 第一阶段只开放 `canvas_context` 和 `canvas_read`。
- 在 Claude Code 与 Codex 中各建立一个测试片子目录配置。

通过标准：两个 Agent 在片子目录都能读取同一项目、同一 revision；在管线层和无关目录启动时不出现无限画布工具。

### 阶段 2：安全写入

- 加入 `canvas_mutate`。
- 复用现有 dry-run、白名单、revision、幂等和人工锁。
- 验证人工在 App 里改动后，Agent 用旧 revision 写入被拒绝；验证两个 Agent 连续操作时不会相互覆盖。

通过标准：Agent A 写入后，Agent B 能看到新 revision；使用旧 revision 必须返回冲突，不得静默覆盖。

### 阶段 3：媒体与任务

- 加入 `canvas_task`。
- 复用受控 inbox、摘要校验、`local-ref:` 和任务状态。
- 保持付费请求只能进入 `pending_approval`。

通过标准：本地媒体可摄入并在 App 播放；付费生成必须由用户在 App 内批准。

### 阶段 4：项目初始化器

- 实现 `infinite-canvas agents setup --project 当前片子目录`。
- 自动生成第一轮两个 Agent 的项目级配置和短规则，对已有配置做合并。
- 所有写入先展示差异并等待用户确认。

通过标准：新片子目录一次初始化后，两个 Agent 无需重复解释即可使用绑定画布；AI编导 的片子目录里达芬奇 MCP 仍可用。

### 阶段 5：第二轮 Agent 接入

- 先核 Grok、Gemini Antigravity 当前版本的项目配置格式与启用方式。
- 扩展初始化器，不改 MCP 服务本身。

通过标准：两者在片子目录能发现无限画布，在管线层和无关目录不能。

## 十一、总体验收清单

- [ ] Claude Code 在片子目录中能发现无限画布，在管线层和无关目录中不能发现。
- [ ] Codex 在片子目录中能发现无限画布，在管线层和无关目录中不能发现。
- [ ] （第二轮）Grok 同上。
- [ ] （第二轮）Gemini Antigravity 同上。
- [ ] 所有接入的 Agent 读取的是同一个 `canvas_project_id` 和 revision。
- [ ] Agent 切换后能继续上一个 Agent 的画布结果。
- [ ] 人工在 App 里改动后，Agent 用旧 revision 写入必须被拒绝，不得覆盖人工改动。
- [ ] 写入前可 dry-run，旧 revision 不得覆盖新 revision。
- [ ] 人工锁定节点无法被 Agent 修改。
- [ ] 相同 `request_id` 重放不会重复创建内容。
- [ ] App 未启动时返回明确错误，并提示用户启动 App。
- [ ] 找不到绑定文件时 `canvas_context` 返回 `NO_PROJECT_BINDING`，并提示到片子目录启动。
- [ ] MCP 故障时 CLI 仍可操作同一个 Bridge。
- [ ] 付费任务必须停在人工批准边界。
- [ ] 无关目录不加载 MCP 工具和长说明。

## 十二、风险与处理

| 风险 | 后果 | 处理方式 |
| --- | --- | --- |
| Agent 配置格式变化 | 某个 Agent 无法加载 MCP | 统一由项目初始化器适配，执行前检测版本 |
| Agent 在管线层而非片子目录启动 | 看不到画布工具，或找不到绑定 | 短规则写明启动位置；`canvas_context` 返回 `NO_PROJECT_BINDING` 并提示 cd 到片子目录 |
| MCP 暴露工具太多 | 上下文上涨、选择工具变慢 | 第一版固定四个高层工具，新增工具必须有明确收益 |
| 返回整张大画布 | 单次上下文暴涨 | 默认局部读取、分页和 revision 增量 |
| 多 Agent 同时写入 | 覆盖人工或其他 Agent 修改 | 保留 `base_revision`、幂等和人工锁 |
| App 未启动 | MCP 无法连接 | 返回结构化错误并提示启动 App，不自动启动未知进程 |
| 项目配置进入 Git | 其他机器绑定到不存在的本机画布 | 团队项目将绑定文件加入 `.gitignore`；个人项目可选择提交 |
| 规则文件写得过长 | 每次对话都浪费上下文 | 规则只说明触发条件和安全边界，细节按需读取 |

## 十三、最终建议

采用以下组合：

1. **主入口：项目级极简 MCP。** 负责让接入的 Agent 自动发现并可靠操作无限画布。
2. **底层协议：继续复用现有 Agent Bridge。** 不重建数据库和协作状态。
3. **备用入口：保留 CLI。** 用于人工排错、脚本自动化和 MCP 故障恢复。
4. **规则入口：极短 AGENTS.md / CLAUDE.md。** 只告诉 Agent 何时使用、遇到什么情况必须停止。
5. **目录原则：一个片子目录一张画布，Agent 在片子目录启动。** 系列剧再拍一集就开新目录。
6. **产品形态：桌面 App 单一入口。** 网页版下线按 `web-product-sunset-plan.md` 单独排期，不误删桌面 WebView 所依赖的前端实现。

在这套结构下，切换 Agent 不再等于切换工作环境：项目绑定、画布状态和操作协议保持不变，只是换了一个操作者。

## 参考依据

- [Codex 配置层级](https://developers.openai.com/codex/config-basic)
- [Codex 项目级 MCP](https://developers.openai.com/codex/mcp)
- [Claude Code MCP 文档](https://code.claude.com/docs/en/mcp)（2026-09-02 核实 `.mcp.json` 目录发现规则、`CLAUDE_PROJECT_DIR`、`roots/list`）
- 本机 `claude mcp add --help`
- 本机 `grok mcp add --help`
- 本机 Antigravity 自带的 MCP、插件与工作区规则文档
- 无限画布现有 Agent Bridge、CLI 与人与 Agent 协作协议
