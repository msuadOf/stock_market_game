# sweep38：根协作指南、贡献指南与 README 全文复核

日期：2026-10-03。生产源码基线 `b76ece3`，审计工作树 HEAD `4ad5a2e` 为审计 merge，生产代码相同。只新增本记录；没有修改产品、Git 状态或执行构建/测试/长验收。

## 全文范围

| 原文路径 | 行数 | 阅读与别名说明 |
| --- | ---: | --- |
| `CLAUDE.md` | 123 | 全文连续读取；为指向 `AGENTS.md` 的符号链接，仍保留此原文路径作为覆盖入口，不重复认定为另一套协作契约。 |
| `CONTRIBUTING.md` | 91 | §0–6及PR自检清单全文。 |
| `README.md` | 248 | 项目介绍、愿景/路线图、当前能力、开发、产品构建、Linux桌面依赖、3×3矩阵、贡献、许可证全文。 |
| 合计 | 462 | 三个指定文档无省略。 |

已读取 `docs/principles.md`、`docs/open-questions.md`，复核 ADR-0027 与 ADR-0028 全文。后续 accepted ADR 优先：ADR-0028规定普通commit/PR不自动触发工作流，CI仅手动诊断，标签和手动产品链只构建/打包/核验/部署，不执行测试、lint、Clippy或smoke。不能以较早 README/贡献指南描述要求恢复自动CI或在发布链新增测试。

本记录“已有”是指定基线静态代码存在，不表示本轮运行通过，也不替代宿主/工具专项审计。

## CLAUDE.md 逐章条款

| 原文条款 | 状态与现行实现/证据 |
| --- | --- |
| 9–19行：Web优先、可选后端、Tauri、engine解耦 | 主干已有。`apps/web/src/app/useSessionHostLifecycle.ts:6` 通过Host适配器选择远程/本地；`apps/server/src/main.rs:1` 运行相同engine服务；`apps/desktop/src-tauri/src/actor.rs:1145` 用ProtocolSession恢复，engine不依赖React。Stage2多人产品不由此三阶段概述自动扩成当前已承诺。 |
| 21–37行：TDD、防御式错误、诚实 | 协作要求保留，不是需要新增的运行时模块；历史TDD先后不能仅由当前源码推断。`apps/web/src/host/startup-policy.ts:33` 对WASM能力缺失明确报错，`:67/72/77` 分宿主给初始化失败原因；当前真正未闭环的宿主错误/通信边界仍见G01–G05。 |
| 38–51行：A股语义、官方依据、差异建模、简化登记、独立复核 | 工程审查门禁仍有效；`docs/trading-rules.md` 登记真实规则范围与游戏简化。未新增交易制度，本轮不重新联网验证或把文档要求当作已完成全部制度仿真。本轮新增工作记录由父任务统一独立复核。 |
| 53–60行：ADR、开放问题、分层、新依赖、Git流程 | 过程要求。已读相关ADR，未新增依赖，无Git写操作，不需要实现额外自动Git机器人。 |
| 62–67行：Agent工作文件归档 | 本记录遵守 `agents/implementation-audit/exhaustive-review/` 目录；不搬迁正式docs，也不把工作记录当正式规范。 |
| 69–79行：普通10秒、长验收5分钟、多核、K7清理deadline | 技术主干已有。`scripts/run-web-tests.mjs:54` 按CPU分片、`:114` 设置10000ms case timeout；`scripts/run-full-regression.mjs:61` 设置Cargo/Rayon，`:300` 分配多binary线程预算，`:616` 将冷构建/执行分离；外层 `scripts/run-with-deadline.mjs` 管进程树。本轮不运行它们。G39是K7事实比较契约，Q05是scripts正式发现范围，不能由timeout存在核销。 |
| 81–95行：Conventional Commits | 协作格式已有，不是缺失产品逻辑；本轮不提交。 |
| 97–110行：gh、人类确认、禁止弱化断言/吞错/编造 | 过程门禁仍有效；自动发布仅在用户已批准有效标签的工作流范围运行，ADR0028不豁免本地agent任意push/发布确认。没有用网页、网络工具或Git写操作。 |
| 112–123行：导航 | 指向正式原则、测试、错误、架构、ADR与Git指南；与CLAUDE→AGENTS别名一致。 |

## CONTRIBUTING.md 逐章条款

| 原文条款 | 状态与限定 |
| --- | --- |
| §0，9–15行：三铁律 | 同CLAUDE过程要求，不用当前测试存在反推历史TDD执行顺序。 |
| §1，17–28行：Node/Rust/pnpm/nightly、install、test/lint/build | 固定版本一致：`.nvmrc:1`=24.18.0、`rust-toolchain.toml:2`=1.96.1、`package.json:5`=pnpm11.19.0；`scripts/frontend-build-plan.mjs:16` 以nightly-2026-09-05进行wasm-pack构建，`:10` frozen-lockfile。`package.json:14` 根test为full-regression，build只重建Web；新clone仍按README先生成WASM产物。 |
| §2，30–37行：保护main、分支、PR | 人类/Git协作流程，不代表当前代码需要新增自动branch或强制服务端Git校验。不能不查询远端就声称branch protection已配置。 |
| §3，39–60行：提交格式 | 格式要求，不执行本轮提交或重写历史提交。 |
| §4，62–73行：Issue、TDD、本地门禁、PR模板、CI全绿 | `.github/pull_request_template.md` 与Issue模板存在；CI仍有完整手动诊断入口。第73行描述需按ADR0028解释为需要时手动运行的评审证据，不能要求恢复commit/PR自动CI。 |
| PR自检，75–82行：测试/错误/断言/docs/ADR/commit | 人类审查清单，产品代码不存在“checkbox执行器”不算缺口。最新发布build-only不改变局部代码变更必需测试和独立复核。 |
| §5，84–87行：Bug/Feature模板与复现 | `.github/ISSUE_TEMPLATE/` 入口存在；用户能否提供特定bug上下文由对应错误路径审计，不能只凭模板存在核销错误展示缺口。 |
| §6，89–91行：行为准则 | 协作要求，不计产品功能遗漏。 |

## README.md 逐章承诺与生产入口

| 原文位置/条款 | 当前状态与代码证据 |
| --- | --- |
| 1–14行：pre-alpha、整tick失败、panic、v2拒旧档、验收诚实 | 状态表述与有限验收边界保留。`packages/engine/src/session/pipeline/candidate_commit.rs:46` 可失败前置完成后`:98`才提交；`session/persistence.rs:260`检查v2 policy；panic不是业务拒单或可恢复StepFatal。旧“历史语料”要求按已退役工具核销，不能重开缺失历史witness。 |
| 18–34行：Web/可选Server/Tauri与Stage1–3 | 主干已有，Stage2明确“继续完善鉴权”。远程Browser鉴权实际缺口仍G01/G02，Stage3原生包已构建不等于所有桌面交互验收通过。ADR0027明确本轮不新增多人同局，因此愿景中“联机”不登记为当前多人功能漏实现。 |
| 38–49行：撮合/竞价/涨跌停/T+1/费用/存档；同React三Host；测试/类型/E2E/CI | engine交易与Host骨架已有；三宿主atomic restore：Server `actor.rs:1360`先restore成功再`:1363`替换game，Desktop `actor.rs:1145`先restore再`:1151`替换。TS生成/检查有 `package.json:16–17`；Web E2E `apps/web/package.json:11`。存在这些路径不能核销G01–G05/G18–G20。 |
| 51–69行：首次WASM构建、开发命令、build前置、remote表单 | `scripts/wasm-build.sh:5`交给统一frontend-build；`frontend-build-plan.mjs:10/16`安装依赖/构建；`package.json:11–19`对应dev/build/test/E2E/types/lint。`App.tsx:700–702`在DEV只以VITE变量作表单初值；`:727/733`显示启动选择并解析target，未静默把开发初值变生产宿主。 |
| 71–109行：四原生目标、静态-only、各自输出、浏览器本地WASM | `scripts/build.sh:4`纯Server直接shell；其余`:19`走build-targets，Windows `scripts/build.bat:3`纯Server走batch/PowerShell。`build-targets.mjs:86/101`分Desktop/Server+web-ui feature，`:103`匹配services。`apps/server/src/deployment.rs:176` 分Server/WebUi/All router。纯Server不依赖Node的事实由shell入口保证，Node规划器存在不构成反证。 |
| 111–125行：平台依赖、多核、拒覆盖、自定义输出 | `server-build.sh:6`自动getconf cores、`:31`检查输出direct child，`:53`拒覆盖、`:68`导出Cargo jobs；UI `build-targets.mjs:77`传jobs、`:134`拒已有输出。固定Node/pnpm/nightly/wasm-pack/Tauri依赖由构建前置与CLI检查管理，不能dry-run冒充实际安装。 |
| 127–135行：未签名安装包、便携归档、Actions artifacts、共用编排 | 原生安装包和归档路径已有：`build-targets.mjs:96`传no-sign；`scripts/package-distributions.mjs`生成便携归档；`.github/workflows/distributions.yml:77`共用frontend-build，`:227`直接no-NodeServer构建。第130行“不自动创建Release”已落后ADR0028，见文漂记录。 |
| 137–192行：GTK/WebKitGTK/pkg-config、Wayland/Xvfb、仅前置不证明测试通过 | 系统环境说明不是产品功能。Desktop Cargo使用Tauri依赖，与平台构建前置一致；旧手动cargo test示例不是正式完整回归supervisor入口，不运行该命令，亦不因示例无deadline就宣称当前正式测试工具没有deadline。示例xvfb命令重复属编辑问题。 |
| 194–223行：3×3矩阵、dry-run/host/all限制、有限NSIS交叉/不支持平台 | `scripts/desktop/build-matrix.mjs:50`拒normal --host、`:269`拒normal target all；`:125`允许NSIS-only实验路线、`:137`标unsupported。主四目标构建只走native，旧矩阵实验路线没有被README说成跨平台安装包普遍可用。 |
| 227–240行：协作必读/TDD/错误显式 | 同根守则，不新增产品功能。 |
| 244–248行：MIT | LICENSE及ADR0007已指定MIT，不要求增加license header或变更授权。 |

## 文漂与漏实现的分界

1. **README:129–130的分发/Release描述已落后批准变更。** 原文说分发只上传Actions artifacts、不自动Release；ADR0028明确有效标签自动Release+Pages，实际 `.github/workflows/release.yml:5`监听标签，`:26`校验tag+SHA，`:41`发布job调用`:63`的publish-release脚本。这是旧README应同步的文漂，不是“自动Release漏实现”，也不以此要求产品发布链加测试。
2. **CONTRIBUTING:73的CI合并要求是流程文案不明确。** `.github/workflows/ci.yml:6`仅workflow_dispatch，与ADR0028相符，仍在`:182/186/201/213/219/227`保留构建、完整回归、Clippy、Web lint与E2E。不能把停止自动CI算作漏实现。手动门禁实际是否运行须按每批证据报告。
3. **README:23愿景中的联机/持久化不自动扩大当前范围。** ADR0027明确不新增多人同局；Stage2远程同一账户与Save API已有，数据库/云档仍由开放问题定义未来范围。无当前多人同局漏实现新增项。
4. **README:48“本地/文件存档”按日终Archive解释。** 当前后续ADR0025只允许公共日终存档，旧v2 runtime quiet-point保留为内部恢复事实，不能要求重新加公共日内保存按钮。存档入口/隔离实际缺口仍归既有宿主及存档专项。
5. **保留G26。** `apps/web/package.json:9` lint仅oxlint，手动CI `ci.yml:219`亦执行此入口；README写“lint门禁”不能证明warning-as-error完成。根TDD/防御式守则同样不能自动核销G01–G05/G21/G39。

本轮没有确认新的产品漏实现；确认两项后续政策下的根文档漂移候选，已给出实际实现反证。版本、四产品模式、启动选择、原生/no-Node构建与多核监督主干存在；真实运行质量和正式验收仍按已有G/Q及专项报告追踪。
