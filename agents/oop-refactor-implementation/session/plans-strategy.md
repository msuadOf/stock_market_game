# plans / strategy 实施与交接记录

日期：2026-10-03。对应 action：`engine-strategy-01-A01`、`engine-strategy-03-A01`、`session-R2-N05`、`session-R2-N12`。

## 已实施范围

- `TradingPlan` 的字段限于 `pub(in crate::plans)`，外部改用同名只读 getter。`code()` 返回 `&StockCode`；其余字段（含 `review`、`last_revision`）返回原类型的 Copy 值。不提供 raw setter 或可变复核借用。
- 五个 free transition 改为 `TradingPlan` receiver，guard、字段赋值顺序及错误保持原样，包括直接修订发生 `VersionOverflow` 时既有部分写入行为。`PlanBook` 继续拥有索引和原子事件批次，委托单个计划记录复核事实。
- `review_preview(direction, confidence_bp)` 只返回克隆副本，保持原来紧迫度评估临时覆盖两个字段的语义，不创建修订事件。
- `ZiNoiseStrategy::strategy_data` 私有投影复用原三个入口的完整散户参数集合，serde、构造、校验和 RNG 注入接口不改。
- `BeliefBook::observe_institution_account_risk` 及私有 `assess_institution_account_risk` 接回 frozen policy、个人 experience 与 pause latch。session 旧 helper 暂作委托，raw pause setter 删除。无 peak 时 `Option<bool>::None` 保持原 latch；真实失败仍可触发暂停；触发 `>=` / 恢复 `>` 与衰减保留。
- `RetailDecisionContext` 是追势抽样通过后构造的单次借用值，集中该股票的完整交易分钟变化、本人持仓成本收益与 stop/dip 门槛，纯信号分类不使用 RNG。Intent、含费资金缩量与可卖数量继续由原决策/sizing 路径处理；噪声卖出仍重新选取可卖股票。
- 按 parent 要求，`CandidateTargetProposal` 在 `candidates.rs` / `plans/mod.rs` 重导出；`assess_institution_behavior` 限于 `pub(in crate::session)` 供纯 root observer 调用。

## 行为边界

依据为既有 ADR-0006、ADR-0011、ADR-0012、ADR-0013、ADR-0015、ADR-0021、ADR-0026 和 action-index 当前正文。此次只迁移 owner 边界，不定义或修改交易制度。

- Money 仍以分、股数仍以股、比例仍以 bp；真实成交计进度，不以接受委托冒充成交。
- 买入整手、卖出既有零股规则、T+1 的 `sellable_qty` 限制、Highest/Lowest 意图和费用资金能力保留。
- 合法 `stop_loss_threshold=0` 及无有效成本时原 `loss=0`、`>=` 可产生 Sell 的行为保留，未混入行为修复。
- serde 字段名、精确浮点表示、存档接受范围与 RNG 调用次序不改。

## 测试与验证状态

新增测试先于方法实现写入；parent 禁止子 agent 运行 cargo/产品测试并统一安排编译窗口，因此未执行真实 red，也未执行 green。缺失新方法预期编译失败只作为接口设计预期，不登记为实际失败测试证据。

交给 parent 的短测试 filter：

- `plans::state::owner_tests`：review_preview 只覆盖 direction/confidence，其余序列化事实与原计划不变，临时值 serde 往返。
- `strategy::beliefs::risk_owner_tests`：无 peak 保持记忆、真实失败触发与衰减不删事实、暂停/恢复等号边界、负 equity / 非正 peak / 时钟回拨不修改 latch、policy/pause serde 往返。
- `strategy::retail::decision_context_tests`：止盈仅用可卖股数、无/零/负成本、stop/dip 阈值与盘口失衡、T+1、零 stop、分钟历史不足、噪声卖出重新选股与完整预设 RNG 消费。
- 既有 `session::institutional_behavior::tests` 已改为通过 owner 评估风险，原断言保留。
- 既有 `plans` integration suite、`strategy` / `strategy_state` integration suite、PlanBook 原子事件测试仍需统一跑。

已执行：所有改动 Rust 文件精准 `rustfmt --edition 2021 --config skip_children=true` 成功；只读全文核查、调用搜索与 diff 检查。没有 cargo、全仓 fmt、Git 写操作或自行创建 subagent。独立复核由 parent 统一安排，本记录不宣告门禁通过。

## 跨文件 API 迁移交接

1. session / persistence / protocol / hash / integration tests 的 TradingPlan 字段读取改为同名 getter；`plan.code.clone()` 改为 `plan.code().clone()`，需要借用时直接 `plan.code()`。
2. decision_chain 的 `reviewed_plan` clone + 直接 direction/confidence 赋值改为 `plan.review_preview(direction, confidence_bp)`。本 agent 没有修改 decision_chain。
3. decision_chain 可直接调用 `belief.observe_institution_account_risk(equity, moment)`；旧 session helper 保留兼容委托。没有新的非机构拒绝条件。

## 追加承接的 caller 迁移

parent 后续分配的以下 session 文件已机械迁移 `GameSession.state.F`，Account 的 cash 读取改为 getter、缺策略非法 fixture 改为已有 `cfg(test) fixture_set_strategy(None)`；保留原断言及非法输入意图：

`hash_contract_tests.rs`、`failure_tests.rs`、`player_candidates.rs`、`player_candidates_tests.rs`、`continuous_cancellation_tests.rs`、`envelope_projection_tests.rs`、`envelope_projection_hydration_tests.rs`、`reconciliation_plan_phase_tests.rs`、`reconciliation_plan_tests.rs`、`plan_chain_candidates_tests.rs`。

`institutional_behavior.rs` 的保留 session adapter 同时迁到 `state.accounts` / `state.day` 和 `Account::positions()`。上述追加文件未出现 belief/candle/attention 或 ParentOrderPlan raw fixture 需要额外迁移。

## Domain N07 adapter 接线

收到 parent 追加指示后，`institutional_behavior::observe_institution_position` 原生产 body 改为薄委托 `experience.observe_institution_position_dated(code, price, moment, policy.adverse_move_threshold_bp())`。原签名、policy 装配边界与错误顺序保留；实际状态写入由 domain agent 实施的 `RetailExperienceState` receiver 完成。既有机构观察测试继续通过该 adapter 调用；未在本 agent 运行产品测试。

追加静态核查：`failure_tests.rs` 的非法行情 fixture 改用 domain 提供的 `fixture_set_last_price`，保留 `i64::MAX` 输入；所属其余测试没有旧 Market setter 或 Account/Position raw 字段访问。限定文件的 `git diff --check` 通过。

## 独立复核补测

`review_leaf` 指出 `engine-strategy-03-A01` 的新私有投影 helper 缺少直接完整参数守卫，现有 StrategyData serde 测试不能覆盖 ZiNoiseStrategy 投影。已补 `strategy::zi_noise::projection_tests::strategy_data_projects_all_individual_retail_parameters`：使用全部非默认实例参数，逐项断言 arrival/order/chase、dip/stop/take-profit/volume、position_step 与 base_observation；f64 按 `to_bits` 比较，风格身份保留。源码实现不变，精准 rustfmt 成功；产品测试由 parent 统一执行，补测待 reviewer 复核。
