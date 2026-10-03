# luna42：Git / gh 工作流全文与现行工具复核

## 范围与基线

- 目标工作树：`/data1/baiyifan/workplace/stock_market_game/.worktree/implementation-reaudit`。目标产品提交为 `08e4fc75b52a71a3262a8a938c57b44f8b5b4960`；当前 HEAD `a7c7ce357bdc9f88c03633744b2d5815db49e9b2` 是合入审计工作的 merge commit。`git diff --stat 08e4fc7 HEAD -- ':!agents/implementation-audit'` 无输出，产品文件在两基线间没有差异。
- 开工前完整读取根 `AGENTS.md` 与 `docs/principles.md`。指定三份文档逐行从首行读到 EOF；行数以 `wc -l` 和 `nl -ba` 核对。
- 只读核对了 Git / GitHub CLI 本地帮助与实际脚本入口。没有执行 Git 写操作、GitHub API/远端查询、发行操作、测试、构建或产品改动。只新建本审计记录。

| 文件 | 行数 | EOF |
|---|---:|---:|
| `docs/git/AGENTS.md` | 18 | 18 |
| `docs/git/daily-workflow.md` | 22 | 22 |
| `docs/git/initialization.md` | 11 | 11 |
| 合计 | 51 | 51 |

## 逐条矩阵

| 原文条款 | 当前工具 / 流程证据与判断 |
|---|---|
| `AGENTS.md:1–3`：文档为 Agent 定义流程、用户以自然语言委托 | 这是 Agent 协作治理，不是游戏产品功能。Git 2.55.0 和 `gh` 2.100.0 均存在；本地 CLI 本身不会替 Agent 读取这些 Markdown 或自动理解用户授权。 |
| `AGENTS.md:5–9`：先读仓库规则；首次接管读 initialization；日常修改读 daily workflow | 文件路径与读取顺序可用。本轮确实已先读项目宪章再读这些文档；没有“文档自动加载器”或 agent 行为测试的产品承诺。 |
| `AGENTS.md:11–13`：不让用户学习命令，以中文翻译状态和风险 | `git status`、`git branch` 等本地 CLI 能供 Agent 执行，但翻译成自然语言属于对话层职责，不由 Git 或游戏运行时实现。 |
| `AGENTS.md:14`：GitHub 操作使用 `gh` CLI | 当前安装版本 `gh 2.100.0 (2026-09-03)` 的帮助列出 `repo`、`pr`、`issue`、`release`、`api` 等命令。当前发布脚本 `scripts/publish-release.mjs:74–76` 通过 `runBoundedCommand({command:"gh"})` 调用；Actions cache 清理 `scripts/prune-actions-cache.mjs:79–82` 同样使用 `gh`；workflow 在 `.github/workflows/distributions.yml:223` 用 `gh release download`。实现证据吻合“GitHub 操作走 gh”，但本次没有实际调用或验证认证状态。 |
| `AGENTS.md:15`：push、远端仓库、PR、Issue、Release 前须用户明确确认 | 这是项目 Agent 授权门禁，不是 GitHub/`gh` 自带的强制交互政策。`gh repo create`、`gh pr create`、`gh release create` 都提供可直接执行的非交互参数；`gh pr create --dry-run` 帮助甚至提示仍可能 push。因此实际安全边界必须由上层 Agent 遵守，单靠 CLI 不能保证。生产 `.github/workflows/release.yml` 的 tag 驱动发行是预先配置的自动化：它不能证明用户对当前会话中的人工操作作了授权，也不应误当成此条款的违反。 |
| `AGENTS.md:16`：不编造仓库、分支、提交、检查或远端结果 | 属于报告规范。只读本地观察到当前分支 `docs/implementation-reaudit`、HEAD `a7c7ce3`、指定产品提交 `08e4fc7`；未观察远端状态或检查结果，不据此声称其存在/成功。 |
| `AGENTS.md:17`：不把密钥、令牌、`.env` 等加入提交 | 属于提交治理；Git CLI 不会自动识别所有敏感信息，`gh` 能读凭据也不代表它会自动阻止提交。本轮未读取凭据或敏感文件，也未暂存。不能把忽略规则等同于完整秘密扫描。 |
| `AGENTS.md:18`：禁止破坏性 Git 操作规避问题 | 这是 Agent 禁令，不是工具能力限制。Git 2.55.0 的命令目录包含 `reset`、`clean`、`push` 等高影响命令；CLI 不会因本文件而禁用它们。本轮未执行。 |
| `daily-workflow.md:1–3`：文件修改及查看/保存本地进度时适用 | 这是协作流程的触发范围。Agent 必须结合当前任务意图判断；游戏没有相应的运行时需求。 |
| `daily-workflow.md:5`：先查状态、分支、未提交修改 | Git CLI 提供 `status`、`branch`；本轮为避免误写只读检查了状态与分支，看到未跟踪审计目录。该事实不等于远端状态。 |
| `daily-workflow.md:6`：小步拆分，受保护分支实质改动前建工作分支 | 本地 Git 有 `branch`、`switch` 等操作能力；“哪些分支受保护”还可能由远端 ruleset/branch protection 决定，不能仅凭本地名字或 CLI 列表证明。本轮处于 `docs/implementation-reaudit`，未切分支。文档列出的命名约定是项目约定，不是 Git 内建规则。 |
| `daily-workflow.md:7–8`：保护既有工作；归属不清时暂停并询问 | 属于多协作者治理，Git 状态可显示文件变化但不能可靠判断作者/归属。此轮已有未跟踪审计目录，故只新建授权的 `luna42.md`，不操作其他记录；没有暂存或提交。 |
| `daily-workflow.md:9`：按比例验证、不得弱化测试、如实报告失败 | 项目级验证要求。当前是文档审计，无产品代码变化；本次遵照委托不运行测试/构建，报告也明确不声称验证通过。 |
| `daily-workflow.md:10`：检查通过后自动创建本地 Conventional Commit | Git `commit` 命令存在，但自动提交是 Agent 指令，不是仓库机器化流程。该句不能越过更高优先级的“不提交/只写审计产物”授权边界；本轮未提交。历史 `sweep42.md` 对该句的结论——任务边界阻止本轮提交，并非产品缺少提交脚本——仍成立。若把这句理解为无条件要求，需由主审按适用的上位指令解释；这不是游戏功能缺口。 |
| `daily-workflow.md:11`：以用户能理解的语言汇报变更、检查和保存情况 | Agent 汇报职责；本记录按实际情况区分静态审阅、未运行验证和未提交。没有应用层对应物。 |
| `daily-workflow.md:13–20`：push、远端资源、历史变更、覆盖已有内容、改变既有内容的冲突处理前需确认 | 这些确认门槛比工具提示更严格，归项目治理而非 GitHub 的通用强制政策。`gh` 帮助给出了创建/修改资源的命令；本轮没有调用它们。对外不可逆操作是否已获授权必须以用户指令/Agent 的当前上下文决定，CLI 成功不构成授权证据。 |
| `daily-workflow.md:22`：明确“只检查”时不改文件、不提交、不上传 | 任务范围约定。本轮并非只检查文件系统，因为主任务明确指定新建此审计文件；但仍明确禁止 Git 写与 GitHub 操作，故只写该单一工作文件。 |
| `initialization.md:1–3`：首次接管或配置未就绪时适用 | 条件流程，不适用于所有代码任务；本次既非初始化任务，也未验证用户身份配置。 |
| `initialization.md:5`：确认仓库；若非仓库且确定项目目录可 `git init` | Git 2.55.0 有 `init` 命令。本地只读 Git 命令成功，说明当前目录处于仓库工作树；未运行 `git init`。 |
| `initialization.md:6`：检查分支、remote、身份和未提交变更，不预设配置 | 工具具有本地检查能力，但该流程要求的 remote、身份状态未在本轮读取；不能把未检查误报为已就绪或缺失。初始化条件未触发。 |
| `initialization.md:7`：身份缺失则询问；仅本地配置，不改全局、不编造 | `git config` 支持本地/全局作用域设置，但并不自动执行询问或约束作用域。本轮没有读取或写入身份配置，也没有询问姓名邮箱，因为此任务不需要 Git 写操作。 |
| `initialization.md:8–9`：保留已有修改；受保护分支上开始实质改动前创建工作分支 | 与日常流程中协作者保护/分支治理一致。当前状态与本轮唯一文件写入已记录；未创建分支、重置、清理或暂存。远端保护状态未验证。 |
| `initialization.md:11`：报告就绪、未保存工作、缺项及确认边界，不上传/建远端 | 初始化交接规范，不是产品功能。本轮仅报告了可观察的本地仓库事实；没有声称初始化完成或远端就绪。 |

## 发行政策与旧结论复核

- `sweep42.md` 的主要判断仍成立：三篇文档是 Agent 治理，不是游戏功能需求；不会因为不存在自动 Git agent、init 脚本或本地自动提交器而产生产品缺口。明确委托的审计文件写入不授权暂存、提交或远端操作。
- 关于 `gh` 现行能力的旧证据可以加强：本环境当前安装 `gh 2.100.0`，本地帮助确认发行创建、下载、查询/修改发布及通用 API 命令存在；源码中的 release/cache/download 调用也仍使用 `gh`。`sweep42.md` 提到 `gh` 使用的结论保留，但其工具版本背景未注明；本报告记录本机版本，不推断所有开发环境都相同。
- 关于自动化发行和人工确认，旧结论应保持严格边界：项目授权规范约束 Agent 主动发起的操作；CI tag 触发的 release 是仓库预配置的机器流程。不能用它证明 Agent 可以不经确认创建/发布资源，也不能要求生产发行 job 模拟对话弹窗。
- 发布代码的当前可核证边界：`scripts/publish-release.mjs:83–84` 在收集资产前校验 tag SHA；`:86–88` 创建 draft；`:93–107` 核对 draft identity、资产清单、大小与 SHA-256；`:109–113` 发布前再次校验 tag SHA，再将 draft 公开。对应脚本操作使用 `gh`。这一静态顺序支持旧记录“不重开 G27”的结论，但本轮未运行脚本测试、未连接 GitHub，不作线上政策兼容、真实发布成功或竞态原子性保证。
- **最新发行政策的限制：**本轮没有网络访问，故未独立查询截至 `2026-10-03` 的 GitHub 官方最新 Release/Actions/Token/权限政策；本机 `gh --help` 证明 CLI 支持的语法，不等同于服务端政策核验。产品审计如要求“最新官方政策”作为依据，应将此点视为未核验项，不得把历史 ADR 或安装版帮助描述成官方政策审查结果。当前三份文档主要定义本仓库自己的审批与工具选择规范，未见声称符合某一版 GitHub 官方发布政策。

## 候选与结论

- 未确认新的独立产品代码遗漏；没有新增 G/Q 编号，也不重开历史 G26/G27。G26 的开发 CI warning 门槛与本三份 Git 治理文档不同范围；G27 的发布前 tag SHA 防护见上文。
- 新的治理观察：文档要求人工对外操作确认，而可执行的 `gh` 接口允许非交互资源变更，确认门禁只能由 Agent 层落实；应在后续对 Agent 执行规则做一致性审查，但不应伪装成应用功能或 CLI 具备的强制保护。
- “自动本地提交”仅可在当前任务授权、上位运行规则及用户约束允许时执行。本轮禁止 Git 写，故未提交；这是遵循边界，不是未完成产品实现。
- 全文行数矩阵覆盖 51/51 行。交易语义不受本次纯治理文档审计影响；没有测试、构建、远端状态、身份设置或最新 GitHub 官方政策验证。
