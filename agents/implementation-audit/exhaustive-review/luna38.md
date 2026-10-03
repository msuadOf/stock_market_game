# luna38：协作守则、贡献指南与 README 全文审计

日期：2026-10-03。目标产品 `08e4fc7`，审计 merge HEAD `a7c7ce357bdc9f88c03633744b2d5815db49e9b2`（产品相同）。已先读 `AGENTS.md` 与 `docs/principles.md`。全文逐行 EOF 阅读：`AGENTS.md` 123 行；`CLAUDE.md` 123 行，是指向 `AGENTS.md` 的符号链接，故是同一内容而非第二份规范；`docs/principles.md` 92 行；`CONTRIBUTING.md` 91 行；`README.md` 248 行。对代码仅作静态入口/caller 核对，没有运行测试、构建或产品，没有 Git 写操作；只新增本文件。

## 章节矩阵

| 文件章节/行 | 原文承诺与当前入口核对 | 判定及旧结论复核 |
|---|---|---|
| `AGENTS.md` 项目与三阶段 `:9–19` | Web-first、Stage 1 本地、Stage 2 可选 Server、Stage 3 Tauri；Rust Engine 作为 `packages/engine` workspace member，由 Web Worker/WASM、Server 和 Tauri 壳消费。README `:38–49` 列明同一 React 应用通过 `EngineHost` 接三类 host。 | 架构方向与当前 caller 一致。Stage 各自的实际完成度不由这段协作规则证明；README 的路线图和 build 文档另作产品范围。 |
| `AGENTS.md` TDD/错误/诚实 `:21–36`，principles 原则 1–2、5–8、总结 `:8–34,53–75,90–92` | 根 `package.json` 有 `test`、`lint`、`types:check`、`test:e2e`；`pnpm test` 指向 `scripts/run-full-regression.mjs`；工程错误边界在 web SaveRepository / host / engine 类型化结果等调用链落实。 | TDD 先后顺序与诚实是过程规范，无法从最终源码证明某次实现是否先写测试；不把无法回溯的过程证据登记成缺失功能。未发现这些章节要求恢复已退役的语料比较工具。 |
| `AGENTS.md` 大 A 与独立复核 `:38–51`、principles 原则 9 `:77–86` | 约束对代码/测试/文档/API 的领域语义，审计自身同样受约束；本批文档、构建入口没有改变交易代码或金额/股数概念。 | 无语义变化，不需要扩展交易规则判断。规则符合性仍由具体交易改动按官方来源复核；这不是官方交易制度核验任务。 |
| `AGENTS.md` 开工/工作文件/多核规则 `:53–78` | 指向 ADR、open questions、architecture 与 `agents/<topic>`；测试 deadline/CPU 规则由 `scripts/run-full-regression.mjs`、`run-long-validation.mjs`、`run-with-deadline.mjs` 等具体入口承接。README `:120` 的 `--jobs` 适用于构建脚本。 | 目录与流程承诺本身不是生产功能。此轮没有启动测试/长任务，故不声称验证资源利用或测试实际耗时。 |
| `AGENTS.md` 提交/GitHub/绝对不要/导航 `:81–123` | Conventional Commit 例子；GitHub CLI 限定；文档导航指向当前路径。 | 与本任务只读产品和仅新增 agent 工作记录相符。CLAUDE 别名没有独立正文差异。 |
| `docs/principles.md` 原则 3–4 `:35–51` | `packages/engine` 不依赖 React/DOM；Web 的 LocalStorage 存档经 `apps/web/src/save/save-repository.ts` `CompressedLocalStorageSaveRepository`，由 `App.tsx:getBrowserSaveRepository`/`useSaveCommands` 调用；文件存档走 `save-file.ts`；pause preferences 独立封装在 `config/pause-preferences.ts`。 | 核心/外壳与存档访问入口可追溯。LocalStorage 存档、文件存档和 session preferences 是不同能力的受控接口；没有据此发现散落直接读写。文件读取也命中测试代码，不误判为生产数据路径。 |
| `CONTRIBUTING.md` 守则/环境 `:9–28` | 版本要求与根 `package.json`、`.nvmrc`、`rust-toolchain.toml` 相符；本地常用命令确实在 package scripts 中存在。WASM 构建入口为 `scripts/wasm-build.sh/.bat`，内部调 `frontend-build.mjs`。 | 本地质量命令仍可由开发者执行；ADR-0028 build-only 限制发布及手动产品构建，不等于删掉本地测试命令。 |
| `CONTRIBUTING.md` 分支/PR `:30–37,62–82` | 原文称 main “始终可运行、测试全绿的受保护分支”，并要求 PR 至少一次评审 + “CI 全绿后合并”。但 `.github/workflows/ci.yml:6–7` 只接受 `workflow_dispatch`；ADR-0028 `:9–18,33–35` 明确普通 commit/PR 无自动工作流，CI 是手动开发诊断，产品/发布不调用。 | **文档漂移候选 D38-01**：这两处把测试/CI 说成分支保护或合并必需条件，与现行手动 CI 决定不一致。代码/工作流没有缺失：当前 `ci.yml` 正是唯一手动 CI 入口；此处需要澄清贡献指南，而不是恢复 PR 触发。手动 CI 可由人发起诊断，但不天然成为该 PR 的必需状态检查。 |
| `CONTRIBUTING.md` Issue、行为准则 `:64–73,84–91` | Issue 模板现存于 `.github/ISSUE_TEMPLATE`；PR 模板现存于 `.github/pull_request_template.md`。 | 模板路径可用。`CI 全绿后合并` 仍归前项 D38-01；Issue 优先、评审数量属于项目流程选择，不与 ADR-0028 的 workflow trigger 决定冲突。 |
| `README.md` 愿景/路线图/状态 `:1–49` | `EngineHost` 在 Web 启动组装中选择 Worker/Remote，在 Tauri host 由桌面桥接；交易规则链接到 trading-rules。`.github/workflows/ci.yml` Windows/Ubuntu matrix 仍存在但仅手动触发。 | `README:49` “Windows + Ubuntu CI”按“存在这些平台的手动 CI”解读为真；它没有写自动 PR CI。建议同步注明“手动”，避免与旧的自动门禁印象混淆，属低优先级文档澄清 D38-02，不是 CI 功能缺失。状态/范围标签不等同本轮运行验收。 |
| `README.md` 本地开发 `:51–69` | `pnpm test`、`lint`、`types:check`、`test:e2e` 与 `dev` 均是实际 script；`pnpm build` 为 web build；前端 WASM 包由 `wasm-build`/`frontend-build` 产生，`cargo run -p server` 对应远程服务入口。 | 当前本地 caller 与指令相符。不能把这些开发者命令外推成标签发行或手动产品构建的门禁。 |
| `README.md` 产品目标/依赖/打包/CI `:71–135` | `scripts/build.sh/.bat` 暴露 `desktop|webui|webui-server|server`；server 在 shell/batch 中分流到无 Node 的 Rust 构建，UI 流程调用 `frontend-build.mjs`；分发脚本打包、CI workflow 上传 artifacts，发布 workflow 另发布。README `:130` 表示 build 不跑全回归，“CI 测试门禁独立保留”。 | build-only 与 ADR-0028 符合；“门禁”一词若暗示普通 PR 必需状态检查则不准确，手动 CI 独立保留但并非自动门禁，建议与 D38-02 一并改为“手动开发诊断”。其余目标、无 Node 部署端、产物路径/`--jobs`、不签名、独立打包阶段均可由脚本/workflow 找到入口。此前 `luna12` 已复核发布构建与 CI 分离，保持结论，不把不跑测试误报为代码遗漏。 |
| `README.md` Ubuntu/无头/矩阵 `:137–223` | GTK/WebKitGTK 依赖说明和 Wayland/Xvfb 命令属于环境说明；矩阵 launcher 为 `scripts/desktop/build-matrix.sh/.bat`，规划模式仅 dry-run。 | 文案明确这些命令不证明本机已安装/测试通过，不存在结果冒充。未运行图形路径；对包名或真实桌面行为不作额外验收声明。 |
| `README.md` 贡献/许可证 `:227–248` | 列出的 AGENTS、principles、testing、ADR、Git 工作流文件存在；MIT 许可证及 ADR-0007 路径存在。 | 与仓库入口一致。未发现新实现缺失。 |

## 旧结论与新候选

- 接受 `agents/implementation-audit/reaudit-tools.md` 与 `luna12.md` 对 ADR-0028 的结论：标签 Release 仅构建、打包、核验、部署；测试/lint/Clippy/E2E 留在独立手动 CI。当前 `release.yml` 标签 caller 与 `ci.yml` workflow_dispatch caller 支持此结论。不能据旧 CONTRIBUTING 中的“CI 全绿”措辞要求把 CI 重新接入 PR 或发布链。
- G26 仍是“手动开发 CI 的 oxlint warning 未设为错误”的既有工具缺口（`reaudit-tools.md`）；它不是发布 build-only 的遗漏。README 声称存在 Windows/Ubuntu CI 也不构成 warning-as-error 已满足的证据。
- 新候选 D38-01 为贡献指南中的自动化/合并流程文漂；D38-02 为 README 对 CI 性质的轻微歧义。二者都是规范文字和实际调用关系的漂移，不是产品代码缺陷。本任务授权仅增加审计记录，未修改正式文档。
- `CONTRIBUTING.md:27–28` 与 `:67` 的本地测试/lint/build 建议不认定为漂移：新 ADR 并没有禁止开发者本地验证，只排除了普通 PR 自动触发和产品/发布流水线代跑测试。
- `principles.md` 中 TDD、显式错误、最小数据访问层等原则不能仅凭约定文字证明历史过程全遵守；本次确认可定位的主要 LocalStorage 存档 caller 受仓储封装，未将无法证明历史 TDD 的事实伪装成漏项。

本轮为静态全文与入口核对，未执行验证命令；没有更改 A 股交易语义或产品文件。建议总审计去重后把 D38-01 纳入文档修订清单，并视需要将 D38-02 合并处理。
