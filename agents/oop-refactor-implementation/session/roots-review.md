# Session roots 独立复核

日期：2026-10-03。复核者：`/root/implement_session/review_roots`；未参与实施。
baseline：`b89afb3346743a4b4fccf26c9ac9ff108595f696`。

结论：指定 roots 范围的三项**独立静态复核通过**，有效发现均已修复并再次核对。
本记录不表示编译、短测试、完整回归或长期验收通过；本复核者未运行 Cargo 或产品测试。

## 范围与阅读方法

动作：`engine-session-03-A01`、`session-R2-N03/N08/N09/N11`，以及 N04 的 roots caller 接线。
已核对 challenge `action-index.md` 中上述完整动作正文、`relationships.md` 的个人观察/风险/续作约束、
实施分配与 `session/roots.md`。前置阅读包括仓库 AGENTS、principles、architecture、open-questions，
并核对 trading-rules 与相关 ADR-0017/0018、ADR-0021、ADR-0026 的领域边界。

下表十个文件构成主审范围。完整 tracked diff、两份新增文件 `roots.rs`/`lifecycle.rs` 全文均已审查；
对长 `decision_chain.rs` 的移动主体和所有旧 test 另做完整函数/断言机械比对，并人工核对不同区间。
其他已有文件的未改正文按调用关系选择读取；不把全 diff 审查写成每份既有文件全文阅读。
`personal_state.rs` 的实现独立复核归其他 reviewer，本次仅核对其 take/install 的 roots 接线。

辅助核对包括旧 `session.rs::clone_for_plan_roots` 全部字段、AccountBook clone/validation、
`institutional_behavior` 输入适配器、`DecisionSnapshot::new` 校验、`ready_ingress.rs` 全文、
`AdaptivePlanChainCoordinator` 的 capture/ready/typed outcome 入口与现有 single worker 通知测试。
`plan_chain_candidates_tests.rs` 的完整 diff 只有 fixture 的 `session.state.plans` 读取迁移，已核对。
这些辅助读取不表示审核其他 worker 的全部实现。

## 三项门禁

### 大 A 语义与依据

静态通过。本批没有新增交易制度或策略公式，沿用 `docs/trading-rules.md` 的已登记沪深 A 股基线：
Money 为分、数量为股、bp 为比例；100 股买入整手、全部可卖余额的零股卖出边界、T+1、
合法撤单窗口、证券类别最大申报数量、价格带/笼子与费用路径均未改。
交易所差异与未支持类别仍由配置及正式规则文档表达，CandidateTargetProposal 没有冒充申报接受或成交事实。

官方依据承接 trading-rules 的上交所/深交所 2026 年交易规则（2026-07-06 生效；文档登记
2026-09-22 核对，时间优先于 2026-09-25 再核对），以及 ADR-0021 登记的
2026-09-26/27 官方正文核对。本次没有重新联网读取官方规则，也没有宣称费用表重新核验成功。
费用表既有访问缺口仍保留在正式文档；本批不更改收费。
机构成本候选、风险暂停和恢复继续属于 ADR-0026 的游戏策略假设，暂停只限制买入，不强制卖出或补钱。

### 必要性与最小范围

静态通过。DecisionChainObservation 统一既有共同观察；RootReadContext 去掉部分 GameSession
的空字段伪投影；InstitutionDecisionRoot 持有单账户 PlanPersonalState，输出既有 Lifecycle assessment
和 AccountExecution typed 增项及诊断。生产 root 不持有 GameSession、markets、history、个人 map、
OrderId/seq 游标，不收集或保存 PlanLifecycleAction，不调用路由或持久化。

PlanRootCoordinator 只接管 Empty/Accounts/InFlight 与通知/状态返还；批次继续汇总操作及管理序号。
PlanLifecycleReview 隔离单股票状态机决策，保留独立 quote preparation 与动作应用。
StockRouteCoordination 保留三张 map 的独立生命期；CandidateTargetProposal 聚合连续纯转换，
未增加 trait 层次、共享可变权威副本、依赖或 wire/SaveSlot 变化。

RootReadContext 一度新增全账户 validation，已依 R02 撤回；最后版本仅普通 COW clone，保持旧 root 读取语义。

### 边界、跨层语义与复杂度

静态通过，没有未关闭有效发现。

- root observe/assess/signals 的完整函数主体在去掉 getter、state、模块路径及注释差异后与 baseline 等价。
  个体注意力 RNG、信息获知、NewMaterial/HorizonExpired、经历反馈与 watchlist prune 次序保留。
  canonical StockCode 的技术结果和首错顺序保留；P1 tick/minute/phase 与账户校验仍在捕获适配器。
- Lifecycle 仍在同 account/code 的必要 typed outcome 到齐后执行。FrozenPlanChainObservation 临时交换
  accounts/markets/auction_orders，提供固定 post-P0 equity/held/策略输入；当前 candidate 提供个人 belief、
  PlanBook 和 parent 事实。动作收集后即应用，未重封资源或回补本 tick 预算。
  `assess_one` 中旧 continue 改为返回本股票动作，再按原 BTreeMap 顺序拼接，guard 与 payload 次序保持。
- `pending`、`reconsideration`、`retry_market` 可以分别存在和消费；reconsideration 优先取出时 retry 保留，
  Complete 不要求复核时两类上下文保持旧行为。错 generation 显式失败且不消费 pending；错误 location 保持
  `plan_chain_candidates::adaptive`。同资源阻塞、独立股票继续和 typed 路由反馈仍由既有协调器决定。
- proposal 保持 weight → total_shares/u32 上界收缩 → target_share_quantity 的顺序；raw_qty 表示收缩后
  取整前数量。非整手股本上限、全现金、恰整手与非法权重首错均有新增短 fixture。
  生命周期 caller 仍区分 `target weight failed`/`target quantity failed` 的原 context。
- `decision_chain.rs` 的 54 个旧 test 全部保留。断言差异是 getter/个人 map 迁移、StockCode 借用类型、
  删除已不存在的 snapshot.markets 断言，以及增加 restored-policy 断言；没有削弱业务断言。
  旧 root urgency clone 测试移除的是已废弃部分 GameSession 路径，实际 quote frozen policy 守卫仍保留。

新增测试已静态审查：`root_observation_rejects_tick_minute_and_phase_clock_mismatches`、
`shared_root_context_keeps_own_facts_when_account_execution_order_changes`、两个 `root_coordinator_`、
两个 `stock_route_`、`lifecycle_uses_fixed_account_resources_and_current_plan_after_outcome`、三个 `proposal_`。
固定资源/当前 PlanBook 测试也断言交换后 live 零资源不变。
single worker 协作让步与成功通知正路径承接未改的
`real_root_notifies_before_stock_stream_starts_with_one_or_several_workers`；不重复增加相同 fixture。
既有 filled-child cancel reconsideration、不可撤阶段、整手/零股、费用与机构隔离测试仍在。

## 发现与复核轨迹

| 编号 | 发现 | 修复与再次核对 |
|---|---|---|
| R01 | Coordinator 提取后残留 `self.operations.append(&mut generated.operations)`，无此字段且 generated 非 mut，不能编译。 | worker 删除内层 append；最终 prepare 返回 generated，只有外层 batch 汇总。已重新读取完整相关 diff，关闭。 |
| R02 | parent 提示后确认 RootReadContext 新 `clone_for_shadow` 校验相比旧 `accounts.clone()` 增加全账户扫描和新的 Fatal/location，debug caller 也会被无关坏策略阻断。 | worker 改回普通 `accounts.clone()`，保留协调 API 的 Result；FrozenPlanChainObservation 既有校验未改。已读最终 capture，关闭。 |
| R03 | worker 自查发现 route outcome 错误构造迁入新 owner 后曾误用 root prepare location。 | 已使用 route_invariant 保持旧 `plan_chain_candidates::adaptive` context；本 reviewer 复核通过，关闭。 |
| R04 | root 最终审查指出 ES03 迁移后的 `GameSession::run_chain_for_account` wrapper 已无 caller，仍留下冗余过渡入口。 | root 明确批准删除该方法与属性，共 29 行；原 reviewer 重新核对 baseline 完整 diff、删除段及全 Rust caller 搜索。原生产 caller 与两个 test caller 均已迁移 InstitutionDecisionRoot，当前无旧定义或 caller；不删除 test、不改公开 API/运行行为，关闭。 |

后续精确 ParentOrderPlan getter、CandleBook、attention scheduler、诊断与 StockCode 借用编译修复已增量审查。
仍有 test caller 的旧 observation/批次构造包装器限制为 cfg(test)，生产 callers 已切换到新 owner；
无 caller 的 run_chain_for_account 过渡 wrapper 按 R04 删除，全部实际 test 保留。
本 reviewer 只写本记录，没有修改源码、运行 Cargo、Git 写操作或创建 subagent。
限定 tracked `git diff --check` 已通过；新增文件全文已另读，不能被 tracked diff 的遗漏掩盖。

## 最终文件绑定

下列 SHA-256 是修复 R02、完成精确 caller 迁移并通知冻结后实算的源文件内容；任一漂移需增量复核。
R04 复核只更新 decision_chain.rs：将删除的 wrapper 精确重插于当前文本后，SHA-256 与上一签署的
`a24654a76aff7302916d9f36e41f01e7c7b0d29b45abc6604555c0ed104e6fb9` 完全匹配，证明其余源文本未变。
另外九个文件保持原签署 SHA。原三门静态结论不变；R04 没有运行 Cargo 或产品测试。

| 文件 | 行数 | SHA-256 |
|---|---:|---|
| packages/engine/src/session/decision_chain.rs | 4423 | b6696ad873d44ab39443ea99d21f31e1dd26e658ad447e494f3971da644399d5 |
| packages/engine/src/session/decision_chain/lifecycle.rs | 570 | 3d400b95ce0d698f26f57fae0b77f2fb09a9e827e6415393d169ccdea44d0f4d |
| packages/engine/src/session/decision_chain/quote.rs | 233 | 8d5227f6106de9ba4d35235af124578da68fd9891642ff45052eb531ca63af1c |
| packages/engine/src/session/decision_chain/roots.rs | 496 | e9c03356cd80cc7da54e6e4c82b5aee3a848754c7bb8a35bbaf8e407abbd3a8d |
| packages/engine/src/session/decision_chain/urgency.rs | 174 | 5a861bdc56bd29218d114101e737428f7c3afc1a4b54a6d7cbadbcf037f70551 |
| packages/engine/src/session/plan_chain_candidates.rs | 532 | 7eff20608f6b3642144a6e82cf1718f50a78982f8228dac2788fe6365e4fa48b |
| packages/engine/src/session/plan_chain_candidates/adaptive.rs | 438 | 6fae566e176b623f579cb67e16afbc4fe80c3b3837bf8e3c3849e2587081855f |
| packages/engine/src/session/plan_chain_candidates/adaptive_tests.rs | 439 | df46f999ff142773a4ba686044b214a175c678172121200ed10032bfdac0b945 |
| packages/engine/src/session/plan_chain_candidates/source_tests.rs | 194 | 35d60f4a9652f29c27eccd25334039fb0fab32de5f2df4fc018d00231c7f7a76 |
| packages/engine/src/plans/candidates/targets.rs | 207 | dbc5238a44bf26c7047aa232713bd68c632d40bff043c302b5c065fa5e5564f1 |

运行验证仍由 root 集中完成：编译所有适用 features/test callers、上述新增短测、既有 roots/continuation
和事务原子性用例。没有实跑 red/green 的新增测试不能登记成 TDD 运行证据；短测也不替代完整验收。
