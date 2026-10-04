# G07：Retail 个人分析生产闭环

## 范围与依据

- 对应审计 G07，依据 ADR-0016、公司信息计划 K5 与任务 17/26：身份不强制决定分析能力，基本面估值属于个人，不是共享市场价值。
- 使用现有 `derive_analysis_profile` 的 Retail 风格分布：允许零基本面权重，也允许有基本面分析能力；不把所有 Retail 改成 Institution 执行系统。
- 本批不改变 A 股撮合制度。买卖继续由原 `RetailPositionDecisionContext::target_for` 和 `retail_position_decision_to_intents` 处理：金额为分、数量为股，买入受现金、费用和整手约束，卖出受本人可卖数量与 T+1 约束。制度依据沿用 `docs/trading-rules.md` 的既有适用范围，不宣称重新查询或改变交易所制度。
- Q25 仍不要求同账户两份 profile 的具体风格全等；恢复只验证身份类别与专用 Institution 字段的合法性。

## 生产路径

1. `populate_npcs` 为 Retail 创建原有四成员个人状态，使用独立 `analysis-profile`、`belief-assumptions` 随机流。只有 Institution 抽定 `InstitutionExperiencePolicy`。
2. `capture_decision_snapshot` 在 accepted attention 与当轮真实 Retail 经历观察后，调用 `capture_retail_analysis`；未被接受的账户不获知新材料。
3. 每个账户独立并行完成发现、watchlist 与 price memory 更新、显式信息获知和个人估值更新。候选保护与修剪使用持仓 ∪ active plan。报告同轮只投递本人最新 period/version，季度/半年报告复用 G09 的非年化补充材料语义。
4. 基本面、趋势、量价、技术、成本经历由本人 `AnalysisProfile` 权重混合。成本经历仅消费对应成本/经历理由，不把趋势或随机 baseline 当作成本信号；本轮真实失败/获利通过共享 `apply_personal_experience_feedback` 更新个人信心，每笔经历订单只消费一次。
5. `DecisionAccountInput` 封存个人候选 assessment 和日期衰减后的失败影响。权威 `RetailExperienceState` 的历史不删改；P2 只在局部 clone 上消费有效影响。
6. `run_npc_decisions` 保持原 Retail 的随机到达、风险与执行参数。个人混合分析改变非风险方向、目标或等待；原风险减仓、T+1、退出冷却、低信心与无到达保护优先。仍不创建 Institution parent-order 执行能力。
7. 现有存档四图保存 Retail 的信息、估值、注意力/记忆。Retail 的 `BeliefBook.experience` 不复制真实成交事实，真实经历仍由 `retail_experience` 单一 owner 保存。恢复初始化只对 Institution 运行机构持仓经历 reconcile。

## 文件归属与跨 owner 接线

- 本 owner：`behavior/mod.rs`、`behavior/heuristics.rs`、`behavior/decision.rs` 的未到达理由、`strategy/beliefs.rs`、`strategy/zi_noise.rs`、`session/decision_chain.rs` 的模块注册与共用经历 helper、`session/decision_chain/retail_analysis.rs`、`pipeline/decision_snapshot.rs`、`pipeline/npc_decisions.rs`、`pipeline/npc_decisions_tests.rs`、`pipeline/decision_snapshot_capture_tests.rs` 的真实 P1→P2 case。
- `implement_personal_strategy`：`decision_snapshot_capture.rs` 的 G07/G08 调用接线、G09 材料更新、G42/G43 候选保护接口。
- `integrate_session_contracts`：`session.rs` 的初始化、机构 reconcile 过滤和 `persistence.rs` 的身份/policy/paused 验证。
- `implement_tick_performance`：`diagnostics.rs` 的 `PersonalAnalysis` → `personal_analysis` 原因映射。
- `integrate_web_save_contracts`：Web strict parser 的 Retail policy/paused 跨层一致性。

## 短测试与诚实记录

- 首先新增 P2 正/负分析方向用例；缺少新 API 的最初编译失败只记录为接口缺失，不冒充行为测试失败。未取得修改前的行为 red，属于不可追认的 TDD 流程偏差。共享其他 gap 的编译暂态错误同样不算测试 red。
- 实际执行发现并修正两处 fixture 预期：T+1 锁定时理由应为 `T1Locked`，不是 `AccountDrawdown`；可恢复测试不得把确定性 profile 的注意力概率改为 1，改为只安排候选 tick 和可重放 RNG state。未放宽或删除库存、风险、恢复校验断言。
- 编译使用 `flock /tmp/stock-market-gap-cargo.lock cargo test -p engine --lib --no-run --target-dir .tmp/gap-target -j16`；编译与测试分离。
- 测试使用已编译 binary、`--test-threads=8` 与命令级 `timeout --signal=KILL 10s`。运行前确认 `--list` 含新用例，不能把旧 binary 的 0 tests 当作通过。
- 新增 13 个短 case，覆盖初始化、实际获知/估值、零基本面权重、未观察隔离、public step/replay、真实 P1→P2、正负方向、持仓卖出、风险/T+1、信息不足、未知股票、失败衰减不删除历史与未随机到达不得造单。
- 最后一个未到达 case 按 TDD 取得真实行为 red：`/tmp/stock-market-g07-arrival-red` SHA-256 `552b08a9316ebca3e51c03a99fbb690b76f8a601cd96ea47e85525d7a62c19bc`，`/tmp/g07-arrival-red-test.log` 实际执行 1 case，`intents.is_empty()` 失败，0.02 秒。之后仅把“缺历史且未到达”的理由改为既有 `NoSignal`，已到达的缺历史仍为 `InsufficientHistory`。
- 最终 green binary：`/tmp/stock-market-g07-final-green`，SHA-256 `e160588a41b9e7baeed6c0fe6d356794b56249701f29927750f529b745fb2e05`，通过同一 `flock` shell 内编译并复制，日志 `/tmp/g07-final-green-build.log`；运行前 `--list` 明确包含 13 个新增 case。
- `/tmp/g07-final-retail.log` 实际 `retail_ --test-threads=8`：71 cases，70 通过、1 个 G08 fixture 日期回拨失败，1.13 秒；本 G07 新增 13 个 case 全部通过。残余 G08 已交 owner，不把整组报告成全绿。
- 原 `behavior` integration suite 的补充编译先被共享 `company_groups/sales.rs` 的新 API 暂态错误阻塞，随后 source 修复后独立 `--test behavior --no-run` 构建成功。`behavior-353d6f66ed1bc96e` 的既有 40 cases 已在 `timeout --signal=KILL 10s`、`--test-threads=8` 下全部通过，0 failed、0 filtered，0.00 秒；日志 `/tmp/g07-behavior-final-test.log`。已到达而缺历史仍保留 `InsufficientHistory` 的两项旧断言通过。未运行完整回归、长验收或性能验收。

## 独立复核

- `review_retail_beliefs` 已审查完整接线，并用 immutable binary 独立执行 `retail_` 短集（70 cases）。发现一个 G07 回归：分析换股后仍把原 ZiNoise 选股加入 `reviewed_stocks`，会复核不应处理的旧报价。已改为只保存最终决策股票，保持现有单股复核契约；等待再次验证。
- Reviewer 要求新增真实 P1→P2、零基本面权重及有持仓负信号卖出用例，均已补齐。真实 P1 fixture 采用公开可见真实买簿和纯量价权重；开局年报缺少正盈利时的基本面 `Unavailable` 不能伪造成可用估值。
- 同次短集发现的 G08 dated writer/legacy fixture 边界已交其 owner 修复；非 Retail 偶然携带 experience 的旧契约仍允许，但不得派生或消费 Retail 专用 `failure_influence`。
- 第二轮发现“没有随机到达仍执行分析”和无条件重构原决定的问题：零到达概率保持原 `StrategyDecision`；分析结果与原仓位决定相等时保留整份原输出；缺历史且未到达明确标 `NoSignal`。原单股复核/取消 case 已重新通过。
- 更正/重述暂沿用现有 `NewMaterial` 入口，Q11 的独立更正 trigger 边界不借 G07 扩大修复。
- 原 G07 的 13 个新增 case 与静态链路已由 `review_retail_beliefs` 独立通过，早期 TDD 历史证据不足仍保留为流程偏差。

## 2026-10-04 scope 增量

- 按 ADR-0016 当日新增政策，报告选择先比较 period，同期间本人已知 `Consolidated` 优先，否则 `Standalone`，同 scope 再比较 version。这是明确的游戏策略，不冒称 CAS 33 要求投资者使用某一 scope。
- 本增量只修改 `retail_analysis.rs`：同轮比较复用 `own_known_report_priority`；有新报告触发时由 `preferred_own_report` 从全部本人已获知报告中选择实际 `NewMaterial` 输入，避免新 `Standalone` 覆盖先前已知 `Consolidated`。无可选报告明确返回 `InvariantViolation`，不静默退回新材料。P2 和失败影响输入不改。
- 中央 helper、同 scope 年报/中期基数及已使用报告的幂等守卫由 `implement_personal_strategy` 实施。
- Immutable binary `/tmp/stock-market-g07-scope`，SHA-256 `a3193c3bffac4061d4bf2720f4a71c0db5fe2b2a9469bda4ed4d37919f55242b`；编译 `/tmp/g07-scope-build.log`，短测 `/tmp/g07-scope-retail.log`。`retail_` 实际 72/72 通过，零失败、零忽略，全部使用 10 秒整命令 deadline 和 8 test threads；之前的 G08 fixture 日期失败已由其 owner 修复。
- `review_retail_beliefs` 与 `review_personal_strategy` 均独立静态确认中央 selector 的 Retail 接线符合上述政策；前者还独立复跑 72/72（1.21 秒）。另建议追加 Retail 跨观察的新 `Standalone` 不覆盖旧 `Consolidated` 生产用例，已请相关 fixture owner 复用其机构报告装配补覆盖；不把共享 helper/机构用例冒充 Retail 专项动态覆盖。
