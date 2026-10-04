# G07 Retail 个人分析独立复核

## 范围与依据

- 复核者未实施本批产品改动。范围为 `retail-beliefs.md` 所列本 owner 文件及 `session.rs`、`persistence.rs`、`decision_snapshot_capture.rs`、`diagnostics.rs`、Web `personal/beliefs.ts` 的必要接线；不以本记录替代 G08/G09/G42/G43 各自审查。
- 已读工程原则、测试规范、架构、开放问题、ADR-0016/0026，以及原审计 G07 的跨身份能力缺口。身份不应强制决定基本面能力；Retail 不应取得 Institution 专用 policy、暂停状态或 parent-order 执行系统。
- 本批未改撮合制度，沿用正式交易规则的适用范围；金额为分、数量为股，执行继续使用原 Retail 转换器的整手、费用、实际现金和 T+1 可卖约束。未重新查询交易所规则，未把策略参数当作交易所制度。

## 静态审查

1. `populate_npcs` 为 Retail 装配个人 `BeliefBook`、信息集、关注列表与价格记忆；profile 与 assumptions 用独立派生随机流，Institution policy 的采样仍仅限 Institution。
2. P1 attention 只对 accepted 账户形成输入；先更新本人真实经历观察，再调用 `capture_retail_analysis`。个人信息获知由本人候选触发，没有休市或未观察账户批量获知路径。
3. 五路信号使用本人 profile 权重；基本面取本人估值而非共同 V，成本信号不把趋势/随机 baseline 冒充真实经历。候选保护与修剪使用持仓和 active plan 的并集。
4. assessment 存入 `DecisionAccountInput` 后由 `DecisionSnapshot` 封存；P2 的 `run_npc_decisions` 读取该输入，不重新访问公开资料。跨身份及未知股票输入有显式校验。
5. `apply_personal_analysis` 保留已有风险减仓、T+1 锁定、低信心、退出冷静期与无随机到达决定；最终订单仍由原 Retail 转换器产生，没有转接 Institution 计划执行系统。
6. 四图存档恢复延续个人状态，Institution reconcile 已过滤 Retail；Rust 与 Web 都拒绝 Retail 的 Institution policy/账户暂停字段。真实 Retail 成交经历仍由 `retail_experience` owner 持有。

当前静态未确认需求外扩展或生产功能缺陷。更正材料的 NewMaterial/Correction 区分沿用既有 Q11 边界，不借 G07 重开。个人 knowledge epoch 的额外历史行为与 G08 等改动仍由对应复核负责。

## 证据与待复核

- 作者先新增 P2 测试，但最初缺 API 的编译失败不满足规范要求的 behavior-red；随后 attention fixture 恢复失败也不是正确原因的需求红灯。必须诚实登记 TDD 流程偏差，不得补写先红历史。
- 作者报告历史暂态通过不可作最终证据：同 target 的共享编译曾覆盖 binary；独立复核仅接受明确绑定的新 immutable binary 和非零 case 结果。
- 当前 `/tmp/g07-final-build.log` 因共享 `ConsolidationFacts` fixture 缺 window 字段编译失败，尚无最终可运行 binary。未运行完整回归、长验收或性能验收。
- 已请求增加真实 P1 capture→sealed assessment→P2 消费实证、零基本面权重边界、持仓负向分析卖出边界；现有公开 step 测试的 belief 非空与 replay 不能单独代替 assessment 消费断言。
- 复核结论当前为等待最终绑定短测和新增边界复核，不宣称 G07 完成。

## 首轮绑定实测

- 作者在同一个 lock 中编译并复制 `/tmp/stock-market-g07-tests`；`/tmp/g07-review-build.log` 编译成功，SHA-256 为 `8c7ab1bd6ceb44fa2cfb82fc9111c996983185b28b5b67f869265f582da3b3f7`。复核者先用 `--list` 确认 12 个新增相关 case 实际存在。
- 独立运行 `timeout --signal=KILL 10s /tmp/stock-market-g07-tests retail_ --test-threads=8`：63 通过、7 失败，1.10 秒，日志 `/tmp/g07-independent-short.log`。这只是代表性 Retail 相关短测，不是完整回归。
- 单独 `retail_analysis::tests` 5/5 通过（0.43 秒），覆盖初始化、本人获知与估值保存、未观察隔离、零基本面权重及公开 step 恢复重放。
- 必须修复：真实 capture→P2 新测试的 assessment 未得到 Scored；non-Retail 体验测试被 `UnexpectedRetailAnalysis` 拒绝，P1 对所有 Some experience 添加 failure influence 与新身份校验发生冲突。
- 另有 5 个共享个人经历相关失败已发送 G08 owner 分类：day 0 观察时钟下溢、本人权益峰值预期漂移、overflow fixture 未触发、reviewed stocks 计数增加、settlement 缺 active entry。未将这些失败隐藏为过滤掉的通过，未修改产品代码或断言。
- 当前结论仍为需要修复和复测，不可核销 G07。

## 修复后复核与最终结论

- 真实 capture fixture 改为本人观察到的真实 player 买簿不平衡与纯量价 profile，不再假定年初年报必有正盈利；Scored、P2 `PersonalAnalysis`、方向和无 Institution execution 的强断言保留。零基本面权重与负分析可卖持仓 case 已加入。
- 修复只重建最终选中股票的 reviewed set；若个人分析没有改变原决定，整份原 decision 原样保留，避免改变原撤单/reconciliation 语义。显式 `arrival_rate == 0` 也保留原决定。
- P1 的 failure influence 仅对 Retail 添加，non-Retail 体验输入恢复既有合法行为。这一修复及原 reconciliation case 在最终绑定 binary 中通过。
- 补充发现并实证修复低正 arrival 下的绕过：缺历史且未随机到达仍被标为 InsufficientHistory，个人分析错误创造买单。先编译红灯 binary `/tmp/stock-market-g07-arrival-red`，SHA-256 `552b08a9316ebca3e51c03a99fbb690b76f8a601cd96ea47e85525d7a62c19bc`，复核者独立重现新 case 的 `intents.is_empty()` 失败（1 case、0.02 秒）。随后 `behavior/decision.rs` 仅把该未到达分支改为 NoSignal；真实已到达分支保持 InsufficientHistory，不增加随机抽样。
- 最终绑定 binary `/tmp/stock-market-g07-final-green`，SHA-256 `e160588a41b9e7baeed6c0fe6d356794b56249701f29927750f529b745fb2e05`。复核者独立执行同一 `timeout --signal=KILL 10s … retail_ --test-threads=8`：70 通过、1 失败，1.14 秒，日志 `/tmp/g07-independent-final.log`；13 个 G07 新 case 全通过，包含上述真正红转绿 case、P1/P2、T+1 风险优先及 save/restore。
- 剩余唯一失败为 G08 `settlement_final_sell_prunes_only_the_affected_retail_account` fixture：entry 日期 2030-01-07，结算日期 2030-01-01，显式 CivilTimeWentBackwards。已交对应 owner，不能把 70/1 表述为 71/71；此项不由 G07 审查核销。

结论：G07 本批产品代码、必要跨 owner 接线及新增边界通过独立语义与必要性复核，没有未修复的 G07 功能发现。A 股约束未改变，没有共同 V、外部补钱或 Institution policy 混入 Retail；实现保持纯 engine 层与现有执行系统。补充根因遵守真实红绿，但整批最初 TDD 历史不足仍为流程偏差，不追认。以上不是全仓源码最终构建、完整回归或长期验收；剩余 G08 fixture 由独立 owner 修复并复测后才能宣称整批通过。

## 2026-10-04 scope policy 增量复核

- 重新读取 ADR-0016 当日新增契约：本人已知报告先按 period，再按同期间 Consolidated 优先、再按 version 选择。这是明示游戏策略，不是 CAS 33 要求投资者使用合并估值；不新增随机 scope 参数。
- Retail 本文件改用 shared `own_known_report_priority` 比较本轮材料，并在 NewMaterial 触发时通过 `preferred_own_report` 从全部本人已知报告选择实际输入。helper 只遍历 `NpcObservationContext::acquired_reports`，未获知报告不参与；已获知新材料却没有本人报告时显式 invariant 错误。
- 没有改 P2、真实经历 owner、交易制度、费用或执行策略。共享更新器的同报告 used-report 幂等避免新 Standalone 获知后重复修订原 Consolidated 判断；helper/会计输入范围由 G09 复核负责。
- 独立绑定 `/tmp/stock-market-g07-scope`，SHA-256 `a3193c3bffac4061d4bf2720f4a71c0db5fe2b2a9469bda4ed4d37919f55242b`。独立 `timeout --signal=KILL 10s … retail_ --test-threads=8` 实测 72/72 通过，1.21 秒，日志 `/tmp/g07-independent-scope.log`；此前剩余 G08 fixture 在此 binary 已通过，不追改旧 70/1 证据。
- 增量静态语义与必要性复核通过，未发现未修复生产缺陷；建议 Retail 自身补跨观察 scope 防覆盖 case，以直接覆盖消费者接线，不能仅以机构 case 泛化所有身份。
