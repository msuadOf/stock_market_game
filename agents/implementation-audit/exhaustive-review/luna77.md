# 发布验证资料全文扫描独立复核

复核对象：产品提交 `08e4fc7`（当前 checkout 为其后续状态）。范围仅限 `agents/oop-release-validation/release-process-checklist.md`、`review.md`、`scripts-final-results.md`；逐行读到 EOF，并对涉及的发布 collector、distribution producer、测试 fixture 与脚本监督入口作源码交叉核对。未运行测试、构建或发布；未改产品文件及 Git 状态。

## 全文章节矩阵

| 文件 / 行 | 逐章内容 | 独立复核结论 |
|---|---|---|
| `release-process-checklist.md:1-3` | 文件身份、检查日期、阅读范围 | 明确是工作记录，不冒充正式规范；日期与任务提交时间相符。 |
| `release-process-checklist.md:5-9` | 实际完整回归边界 | build/execute 分离、必跑 ignored 长例、Web 测试范围及 scripts/lint/build/E2E 排除项交代清楚。 |
| `release-process-checklist.md:11-32` | 本地 CI 等价环境与命令 | 固定工具链、并发和 deadline 说明具体；Rust 普通 case watchdog 缺失另行保留为限制。`run-scripts-tests.mjs` 的每文件与聚合 supervisor 可在脚本本身确认。 |
| `release-process-checklist.md:34-42` | test Release 七项核对清单 | tag/run/source SHA、35 assets、manifest digest、draft/prerelease、Pages/SW、cache 与产品验收边界逐项区分；没有把短 fixture、部署或构建说成完整玩家验收。 |
| `release-process-checklist.md:44-50` | 当前流程缺口 | scripts 未统一入 CI、公开前 tag SHA 仍非原子、collector 未强制完整格式矩阵、E2E deadline/reuse server、Rust 普通 case 时限均明确记录。 |
| `review.md:1-9` | 复核身份与当前结论 | 结论限定于列出的十四个文件与本轮本地提交，不宣称远端发布；提交/用户文件边界明确。 |
| `review.md:11-28` | 范围及读取依据 | 列全十四文件及邻接入口；工作记录仅说明审查依据，不延伸为本代理已复查整份实现 diff。 |
| `review.md:30-36` | 大 A 语义与依据 | 本轮无交易规则变更；历史规则依据及未重新在线核验范围有诚实说明。 |
| `review.md:38-49` | 必要性、等价性、边界复杂度 | Arc 共享值生命周期、错误顺序、fixture 与忽略规则的论证可追踪，未见宣称增加新的交易语义。 |
| `review.md:51-61` | 边界覆盖及 R1 | lifecycle 边界及顺序断言假阳性已修复；绿灯被标明为实施者报告而非复核者重跑。 |
| `review.md:63-87` | 第二轮价格 fixture / tag SHA | 先决受理事实、反控制、符号价格语义、SHA 二次查询及其非原子限制均被说明；没有宣称彻底消除 tag race。 |
| `review.md:89-115` | 第三轮 UI、R2/R3、旧证据 | 公司报告选择和身份边界有解释；R2 fixture 身份修正有闭环。R3 明确聚合验收需共享外部 deadline；失败和复用 preview 的旧 E2E 日志不冒充新构建通过。 |
| `review.md:117-128` | 最终日志核销与提交签认 | full regression、fresh E2E、生产构建、supervised scripts 有独立日志/计数；忽略用例不计通过，且签认明确不是远端发布证明。 |
| `review.md:130-153` | 保留边界与源码哈希表 | 复核者没有运行全量门禁、没有发布的限制清楚；哈希清单覆盖所称十四文件及工作脚本。 |
| `scripts-final-results.md:1-40` | 初始 32 文件脚本测试结果 | 逐项列 32 文件、耗时与日志，批次 14234ms。文件标题“普通测试”属于初始历史批次记录；后续长验收记录为 `scripts-supervised-results.md`，不可将两批合并或只引用旧记录作为当前有监督证据。 |

## 发布 producer 与 collector 边界

工作记录关于格式完整性缺口的旧结论经源码复核成立，而且两层职责不能混称：

```js
// scripts/publish-release.mjs
|| !Array.isArray(manifest.files) || manifest.files.length === 0
```

collector 校验恰有十个 product/target 目录、目录身份唯一、manifest 与目录文件清单完全一致、记录 bytes/SHA-256 与真实文件一致、资产名不冲突；但只要求每个 `files` 非空，不规定 Desktop/Server/WebUI Server 每平台的格式集合。对应测试 fixture 给十个目录各放一个 ZIP，并断言只得到 21 项本地资产；所以这不是推测，而是明确通过的弱格式 fixture。

另一层 `scripts/package-distributions.mjs` 是 producer：Desktop 按 Windows MSI+NSIS、Linux DEB+RPM+AppImage、macOS DMG 及 app bundle 归档检查输入，非 Mac 另打 portable ZIP，Mac app 打 ZIP 与 tar.gz；Server/WebUI Server 各打 ZIP 与 tar.gz。实际 main 发布记录称 35 项资产经下载逐项与 manifest 及 GitHub digest 核对，支持“当次 producer/发布产物完整”的证据。它不能反向证明 collector 自己强制该组合，也不能消除未来错误或被替换 producer 只交非空文件的风险。清单第 48 行恰把 producer 当前约束与 collector 独立防御缺口拆开，结论合理，不应把此缺口误报成当前发布漏包，也不应用当次完整产物替 collector 补充契约。

## 旧结论复核与候选反证

- R1/R2/R3 旧结论均有其自述边界：测试实施者的结果不冒充本次独立重跑；R1/R2 为断言/fixture 修正；R3 对聚合入口加五分钟进程外 supervisor，保留每文件与每 case 的 10 秒限额。当前脚本 `run-scripts-tests.mjs` 可见外层 `runBoundedCommand(... timeoutMs: 300000, cleanupReserveMs: 1000)`，内层仍各 worker 执行 10000ms runner；对应 supervised 记录是 32/32、13675ms、最长 7961ms。
- `scripts-final-results.md` 仍以“普通测试结果”为标题并记录 14234ms；这是旧轮次原始结果，不等于 R3 未修复。它没有声称有外部聚合 supervisor。需引用 `scripts-supervised-results.md` 才能证明修复后工作流；release checklist 第 32 行也明确给出有监督入口。此处属于历史批次与修复后批次并存，非当前实现缺失。
- checklist 所列 deadline 均未超出项目上限：常规 Node case/文件 10000ms，发布/长验收共享 300000ms；K7 最后 1000ms 清理预留由 runner 显式配置。Rust 普通 case watchdog 不足仍保留为未证明项，没有从整阶段低于五分钟推导逐 case 合规。
- 平台格式事实不改变 A 股交易语义。本任务是发布验证工具链复核，无需调用交易所规则；没有修改或模糊交易制度。
- 本范围未发现新的需修复候选。独立防御缺口已有明示且证据支持；历史脚本批次亦可由独立 supervised 记录反证为已补工作入口监督。无需仅因材料中出现旧状态或 TODO 判 G。

## 复核签记

本记录只新增于指定目录。完整逐行扫描的 EOF 位置分别为 checklist 第 50 行、review 第 153 行、scripts 结果第 40 行。未复跑测试；对话中没有据此宣称生产可发布或所有平台 GUI 验收通过。
