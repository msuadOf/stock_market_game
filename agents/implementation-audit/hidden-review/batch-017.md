# 批次 17 全文复核

## 范围与方法

- 来源基线：`43b1aa5`；产品源码取自 `.worktree/implementation-reaudit`。本记录只覆盖清单 batch 17 三个来源，不把来源内引述当成当前实现证明。
- 三篇来源均用连续 `cat` 从首行读到 EOF；现场 `wc -l` 与 `sha256sum` 和 `scan-plan.json` 一致。各篇无截断、无需补读。
- 复核主线：旧材料关注的 Hosts N01–N08，已由现行结构和真实 caller 接入；其中代码 owner 存在不等于所有行为边界已修复。总账把功能缺口 G 与测试/验收边界分开，不能将 OOP 提取或未运行测试计作功能缺失。

## 来源全文矩阵

| 来源及指纹 | 全文结构 | 复核判断 |
|---|---|---|
| `agents/oop-refactor-audit/completeness-2026-10-03/hosts/review.md` — 54 行，SHA-256 `7a2739cd582607498118e98fd771a37194d9bb497b0eaef3f75fa9b1466eb136` | L1–5 复核对象/边界；L7–23 总体结论与294项覆盖；L25–36 N01–N08逐项判断；L38–40 发现与N05权重；L42–49 二次复核；L51–54 身份与指纹更正 | 这是一份对调查报告/候选动作范围的审查，不是候选代码未实现的证据。N05明确可选且低优先级；其他七项结构候选也应区分迁移与行为修复。文中报告、action、coverage 的计数仅是该历史审计材料的静态复核结论。 |
| `agents/oop-refactor-audit/completeness-2026-10-03/pipeline/independent-review.md` — 25 行，SHA-256 `41d16202fb04e1e83f2d93a3944a6aec588098c272613b6ded286d6d05f3a958` | L1–3 范围与方法；L5–10 reviewer 表；L12–13 结论限制；L15–25 管理者来源声明及历史指纹 | 两名独立 reviewer 针对 engine pipeline 候选计划分别认可必要性、边界及交易语义；文中明说只证明候选计划审查通过，不代表实现或测试通过。其文件指纹属于当时被审版本，不能外推到当前报告内容。与 Host 候选无直接代码链。 |
| `agents/oop-refactor-audit/completeness-2026-10-03/pipeline/reader-instructions.md` — 13 行，SHA-256 `e0d1abda9aa9db2b6be7f93d3f791a4eeeffb76ead504a75f34c4664d69b4cf6` | L1–4 范围/必读材料；L6–8 目标与候选判断方式；L10–13 冻结契约与输出要求 | 属于旧 reader 的任务指令，不是产品承诺或 ADR。明确禁止以无状态/service 简化推导类缺失、区分新增/已覆盖/仅改善；pipeline 专属 P0/P1/P9 约束不得挪用为 Host 证据。本复核遵守其反误判原则，未将历史指令升级为现行需求。 |

## 候选与现行代码核对

下列八项为 Hosts review L25–36 所讨论的 N01–N08。**当前源码已包含提议的 owner 与调用接线**，所以它们是已完成结构迁移/既有测试组织候选，不是当前新增实现候选。文件行号均取 `.worktree/implementation-reaudit` 当前源码；行为缺陷另按总账记录。

| 候选 | 现行 owner、caller / consumer 与精确证据 | 反证、关联与判定 |
|---|---|---|
| N01 `DesktopPacing` / `ServerPacing` | `apps/desktop/src-tauri/src/actor.rs:142,151,639,675`；`apps/server/src/actor.rs:308,317,965,1009`。两个 actor 分别持有并更新各自 pace state。 | 两宿主不应合并节奏语义；旧复核指出桌面 1µs、服务端 1ms、semaphore/fastest 与陈旧发布缓冲差异。当前总账 G19仍记录 Desktop 固定高倍率缺少聚合，不可误以为 pacing owner 已修该行为。结构已接，无新增候选。 |
| N02 `StaticAssetRoot` | `apps/server/src/web_ui.rs:18-20,27-64` 保存 canonical root 并验证部署包；`static_router:97-103` 注入 `WebState`；请求通过 `static_file:127-135` 到 `serve_file`。`resolve_resource:72-89` 每次解析/containment/metadata。 | `apps/server/src/web_ui.rs:66-70` 拒绝危险字面路径。原复核要求保留 GET/HEAD、错误区分、symlink/TOCTOU政策；审查未提出更改安全行为。当前 code 有 owner 与实际请求消费链，不是建议待实现。 |
| N03 `Scenario` | `packages/engine/tests/information_acquisition/fixture.rs:128,146`；后续调用路径 `:265,273`，分别构造未披露事实、发布更正；相关 acquisition/failure/view 测试调用它。 | 是测试 fixture，不是生产 acquisition owner。个人 acquired state 仍需显式记录；公开披露不能被 fixture 自动折算为本人已知。旧审查守住未读/version pin/dev query/未来日期边界。G09中期报告缺失与fixture整理是不同层次，不由其核销。 |
| N04 `WeekendScenario` | `packages/engine/tests/publications/weekend_publish.rs:40,47,110,208`；场景方法 `:74-100` 安装、周五 tick、结束自然日、推进 seeded ops、派发。 | 测试输入聚合已接；不属于模拟器生产日历。该测试明确不令周六自动 tick，civil seq/18:00/先读/幂等行为仍由被测链验证。周末公告是合成排期，不构成法定披露规则；报告排除 Q1 法定时限验证。 |
| N05 `SeasonedSaveFixture` | `packages/engine/tests/save_contract/main.rs:116,122,132,140-152`；`seasoned_fixture()` 以 `OnceLock` 保存 baseline，clone 后独立篡改测试。 | 已有共享 immutable baseline owner，新增具名包装收益有限，原审查明确“可选/低优先级”。不能计为当前必需动作。save schema/事实不因 fixture 改变；鲜建 session、两日流程和 clone 隔离保留。 |
| N06 `RejectedCommandFixture` | `apps/server/tests/deployment_cli.rs:5-8,10-57` 持有真实 `Child` 与 deadline；`rejected:59-61` 是测试调用入口，四个 CLI 失败用例在 `:63-80` 消费。`kill_and_wait:25-29` 明确终止并 wait。 | 真实 server 子进程仍以黑盒方式测试；测试 owner 已接。来源特别声明不在 Drop 添加清理政策。panic/异常时 child 的额外清理是行为修复而非提取；不因未增加 Drop 就报生产缺口。 |
| N07 `ProcessSampleRun` | `scripts/simulation/escrow-performance-harness.mjs:325-416` 持有 child、采样与结果；`runProcessSample:418-420` facade；生产链 `:492,497,597` 从 ComparisonRun 到真实 sample。测试调用在相邻 `.test.mjs:326,406,466`。 | 输出仍有 stdout/stderr、peak RSS、thread samples、wall time。来源和后续 sweep65 均指出 sampler reject 时 `start()` 先等 child close 再 await sampler（当前 `:395-400`），可能未及时终止存活 child；这是未修工具生命周期边界，不能冒充已由结构迁移关闭，也不能因此认定采样功能不存在。与性能工具验收相关，不是 A 股语义或总账 G 功能。 |
| N08 `BoundedCommandRun` | `scripts/run-with-deadline.mjs:27-42,44-90,93-147` 持有 child/timers/abort/listeners/settlement；`runWithDeadline:150-155` facade。被 build、publish、full-regression、long-validation、web-tests 等真实脚本导入调用。 | 审查提出 AbortSignal 预先 abort/运行中 abort 与 duplicate close/error/abort single-settlement 的边界；当前 `:40` 预先 abort、`:87-89` 注册/监听，`:106-147` abort 与 one-shot settle 已接。Windows `taskkill` 以 `unref()` 启动后立即返回（`:12-18`），并未等待树退出确认；这是明确未含在 OOP 迁移内的清理/行为边界，不能宣称完全消除。deadline 10s/300s 与 reserve 校验 `:6-8,27-33`保持显式。 |

## 总账关联与新增候选反证

- 当前总账把产品/宿主缺口列为 G01–G05、G18–G20、G40、G53、G66 等；Host pacing/asset-root/test fixture/performance fixture 迁移没有替代这些 caller 行为，也没有新交易制度主张。`G19` 当前行文仍明确固定速率只 `run_cycle(1)` 后 emit，Fastest batching 不代表固定聚合。
- `N07` 的 sampler rejection 清理点与总账产品 G 无关；当前最新扫查已在 `exhaustive-review/sweep65.md:10,36` 标为仍开放边界，未宣称行为修复完成。
- `N08` 的 Windows `taskkill` 未 await 子树终止已由较新审查单独划出，见 `exhaustive-review/sweep66.md:26`（总账 `G66` 为 Remote 请求出口，与该编号无关，避免同名混淆）。
- 较新 `exhaustive-review/sweep64.md:36-40,50-53` 逐项反证 N03/N04/N05 fixtures 确实被测试消费，并说明特定状态/排序没有被封装抹平；`sweep65.md:27,35-36` 反证两个 pacing owners 与 `StaticAssetRoot` 真实存在；`sweep66.md:26,38-39` 反证 N08 真实 caller 与 save/fixture owner。以上是旧候选已经落地的证据，不把状态对象存在扩展成全功能完成。
- 没发现由这三篇来源支持的新增产品 G 或独立 A 股规则候选。未测试的语义边界记录为验证限制，不当作生产实现缺失。对官网现行交易所规则未作外部核验；本批没有新增交易制度断言。

## 未核实项

- 未运行测试、构建或完整回归；这里只读取现行实现、调用者和历史复核证据，不能报告本次测试通过。
- 未重新全文展开 `.worktree/implementation-reaudit` 的全部 Host 源文件或重做 action-index 的全覆盖审计；仅对上述 owner/caller 及最新总账关联点核实。
- 对 `N07` sampler rejection 和 Windows `taskkill` 的进程树最终状态未执行动态反例。历史/现行材料明确将其保留为行为边界，故不将其错误关闭。
