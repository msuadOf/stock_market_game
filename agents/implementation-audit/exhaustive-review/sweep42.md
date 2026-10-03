# sweep42：Git 协作规则与实际工具入口

## 全文范围与操作边界

生产审计基线按主审要求记 `b76ece3`（审计分支合入产品源码同一）。以下三文件均从首行连续读取至 EOF，共 51 行；没有 GitHub 请求、Git 写操作、测试执行或产品修改。

| 文件 | 行数 | EOF |
|---|---:|---:|
| `docs/git/AGENTS.md` | 18 | 18 |
| `docs/git/daily-workflow.md` | 22 | 22 |
| `docs/git/initialization.md` | 11 | 11 |

读取了根 AGENTS.md 与 principles；本批还依指令用只读 `git status --short`、`git branch --show-current` 确認在 `docs/implementation-reaudit`，现有未跟踪目录为 `agents/implementation-audit/exhaustive-review/`。本分片只新建自己的 sweep42，不改写其他分片或暂存任何文件。子目录Git规范是Agent操作规则；不会仅因规则写有“自动创建本地提交”就把当前禁止Git写的审计任务扩大为提交操作。

## 逐章/逐条状态

| 原文与行号 | 分类、当前状态与入口证据 |
|---|---|
| Git AGENTS 目标 3 | 自然语言代办Git是协作运行指令，不是股票游戏或宿主需提供的Git UI功能。无需新增用户态“仓库初始化”或Git服务。 |
| Git AGENTS 读取顺序 5–9 | 操作前阅读根规则/初始化/日常流程；本批已全部读到EOF。不是脚本必须实现的动态文档加载器。 |
| Git AGENTS 总则 11，中文说明 13 | 汇报与用户沟通规范，没有新产品代码要求。 |
| Git AGENTS gh规则 14 | 实际发布脚本 `scripts/publish-release.mjs:74` 通过 `runBoundedCommand({command:"gh"})`；缓存脚本 `scripts/prune-actions-cache.mjs:80` 同样调用gh；打包workflow下载Tauri官方release用 `.github/workflows/distributions.yml:223` gh。没有发现这些实际GitHub操作改用手写HTTP/浏览器。 |
| Git AGENTS 对外确认 15 | Agent发起push/远端创建/PR/Issue/发布之前的授权规则；本次没调用GitHub。已获批的tag发布自动化 `.github/workflows/release.yml:3` 由tag push触发，不意味着脚本必须再添加人工弹窗；不能据此重开已有ADR-0028发行机制。 |
| Git AGENTS 诚实 16 | 不编造状态/结果；旧CI/线上发行记录不能冒充当前实测。本批结果仅为静态读取。 |
| Git AGENTS 敏感信息 17 | 提交前审查是Agent职责；`.gitignore:25`–`:30` 已忽略.env/.env.*/pem/key，`:15` target等产物。ignore不是完整安全证明，但文档没有承诺需新增自动密钥扫描产品或Git hook。没有读取Git身份、token内容或敏感文件。 |
| Git AGENTS 禁止破坏性绕过 18 | Agent安全操作规则。脚本/工作流搜索未见git reset --hard、git clean、git push --force生产入口。TypeScript `--force`是构建选项（`scripts/frontend-build-plan.mjs:22`），不是强制Git覆盖。 |
| 日常流程适用范围 3；步骤1 5 | 要求检查状态、分支与已有修改；本批只读检查已完成。无需实现另一个股票游戏Git状态页。 |
| 日常步骤2 6 | 小批修改与受保护分支工作分支规范；当前审计已在docs工作分支。分支名前缀是操作约定，与 `CONTRIBUTING.md:30`–`:37`一致，不是自动创建分支脚本欠缺。 |
| 日常步骤3/4 7–8 | 不侵犯已有修改及归属不明时停止；属于多协作者操作纪律。共享目录内只新建单独分片文件，不把其他审计记录暂存/提交。 |
| 日常步骤5 9 | 按改动做适当检查、不得弱化测试、诚实报告；规则自身不要求所有每次push自动跑CI。当前手动CI入口 `.github/workflows/ci.yml` 与新发行决定由专门分片核验；前端warning门槛仍属总账G26，不能以通用流程重复开同项。 |
| 日常步骤6 10 | 完成并检查通过后自动本地提交是Agent运行指令，不是应用功能。这一批主审明确禁止Git写，所以未提交；不能把未新增git-commit脚本或本轮未提交计为产品遗漏。 |
| 日常步骤7 11 | 中文交接已完成内容、检查和本地保存状态；操作报告要求，无新代码。 |
| 日常明确确认 13–20 | push/创建资源/历史修改/处理既有冲突需确认；本次全部未执行。没有以“审计”名义改Git remote、仓库保护设置或创建Issue。 |
| 日常只检查例外 22 | 用户只检查时不改/提交/上传是任务限定；本轮任务已明确授权新增审计分片，授权没有延伸到产品或Git写操作。 |
| 初始化范围 3；步骤1 5 | 不是Git仓库时才可init；当前read-only Git命令已表明是现有仓库，不需要初始化或创建新仓库。 |
| 初始化步骤2/3 6–7 | 身份/remote检查与缺信息询问用于首次接管/身份未就绪的Git写工作；本轮不执行Git写，不需要为审计修改或收集身份。不存在要求产品存姓名邮箱、改全局配置的承诺。 |
| 初始化步骤4/5 8–9 | 保护已有工作、主分支修改前切分支；当前已在审计工作分支；没有reset/clean/暂存其他记录。 |
| 初始化完成报告 11 | 报告就绪/缺信息/对外确认，且初始化不上传；属于协作流程，无新产品代码或脚本要求。 |

## 候选与反证

1. **未发现新增独立代码遗漏。** 三文件均为Agent协作规则；总账S02已有“模板/协作规范不算新增产品功能”的相同边界，本批提供具体逐条覆盖。
2. **不新增“缺少自动本地提交脚本/初始化脚本”。** 原文指令的执行主体是Agent，Git CLI现有能力可执行；文档没承诺另建repo-native自动工具，更没承诺股票产品应承担Git管理。
3. **不新增“发行脚本未再弹人工确认”。** 外部操作授权适用于Agent发起动作；tag push→release的获批自动化已由ADR-0028覆盖。本批只核现有入口，没有查询线上或发起发行。
4. **不重开G27。** `publish-release.mjs:109` 在draft与资产核验后再次核tag SHA，`:113`才公开，原发布期间标签变化窗口已补；不宣称这是GitHub原子标签锁，也不重复运行线上发布验收。
5. **G26仍按原总账定义。** 通用“CI全绿/检查通过”流程不能替代手动开发CI frontend warning退出约定；也不授权恢复普通commit/PR自动运行或把lint加进Release链。

结论仅覆盖三份51行Git文档及相应静态工具入口；没有声称远端分支保护、用户身份、GitHub授权状态或本轮CI已实际通过。
