# fixtures1 实施状态

六个动作均已完成源码迁移，等待 root 统一编译、定向测试及未参与实现的 subagent 独立复核。没有运行 Cargo、完整回归或 Git 写操作。

已读取 AGENTS.md、principles、testing、architecture、open-questions 与相关 ADR-0002、0009、0011、0012、0014、0019；本批仅重组测试场景，保留已有 A 股交易规则、单位、沪深差异、T+1、披露与个人获知分离。没有新增制度规则，因此没有进行新的官方规则检索或声称现行规则重新取证通过。

## hosts-N03

- 状态：源码完成，验证与独立复核待执行。
- 文件：`packages/engine/tests/information_acquisition/fixture.rs`、`acquisition_gold.rs`、`failures.rs`、`view_gold.rs`。
- 所有权：原 `Scenario` 继续持有 company/member、Books、ClosingEngine、PublicLibrary、披露时点与 PublicationId；不新增状态。
- 方法：`Scenario::new()`、`post_undisclosed_fact(&mut self)`、`publish_correction(&mut self) -> PublicationId` 直接承接原构造和跨三个 owned 状态的更正流程，删除旧 free function。
- caller：三个测试模块均迁移至构造器及 mutator；`NpcInformationState` 和行情仍由每个测试独立持有，获知仍显式 `record_acquisition`。
- 测试：保留原全部信息隔离、未读、早读、更正版本和 serde 断言。root 精确短筛选：`--test information_acquisition unread_publication_never_enters_unread_npc_inputs`；`--test information_acquisition acquired_version_pinned_across_later_correction`。
- 未完成：运行上述测试与独立复核。

## hosts-N04

- 状态：源码完成，验证与独立复核待执行。
- 文件：`packages/engine/tests/publications/weekend_publish.rs`。
- 所有权：`WeekendScenario` 拥有 year、SeededPrehistory、GameSession、DisclosureDispatch。
- 方法：`new(offset)` 保留 Saturday 确定性搜索；`install_dispatch()`、`step_friday()`、`settle_and_dispatch()` 统一安装、明确周五 tick 与 Session 日结 → 公司经营 finalize → 披露顺序。后者返回真实 `CivilDayEndReport`、`DayEndDisclosures`。
- caller：两个周末金样全部迁移；周六不调用 step，shock 注入、重复派发及全部公布/市场状态断言仍显式留在测试。
- 测试：root 精确短筛选：`--test publications weekend_report_publishes_without_trade`；`--test publications interim_announcement_publishes_at_next_disclosure_phase`。
- 未完成：运行上述测试与独立复核。此处 Q1 排期继续是原合成游戏场景。

## hosts-N05

- 状态：源码完成，验证与独立复核待执行。
- 文件：`packages/engine/tests/save_contract/main.rs`、`failures.rs`。
- 所有权：`SeasonedSaveFixture` 拥有固定 SessionSetup、seed 与只读 baseline JSON 的 OnceLock；外层 OnceLock 仅缓存同一 fixture。没有共享可变 GameSession。
- 方法：`new()`、`build_session(&self)` 鲜建并运行原两日；`clone_save_value(&self)` 惰性生成一次完整 baseline 并为每个篡改测试返回独立 clone。
- caller：主模块 roundtrip/invariant 与 failures 原鲜建/JSON 消费者直接迁移为 fixture 方法；删除 `seasoned_session`、`seasoned_json`，没有恢复缓存档替代鲜建链。
- 测试：新增 `cached_baseline_clones_do_not_share_tampering` 验证 clone 篡改不污染 baseline；原 schema、非法档及源会话不变断言保持。root 精确短筛选：`--test save_contract cached_baseline_clones_do_not_share_tampering`；`--test save_contract new_format_roundtrip_restores_authoritative_state_byte_identically`。
- 未完成：运行上述测试与独立复核。

## hosts-R2-N01

- 状态：源码完成，验证与独立复核待执行。
- 文件：`packages/engine/tests/attention_discovery/exposure.rs`、`failures/discovery.rs`。
- 所有权：`AnnouncementExposureFixture` 拥有 PublicLibrary、公布时点、codes 与从同一代码集合生成的 MarketView；私有 Books 保持独立。
- 方法：`new()`、`exposed_at(as_of)` 每次按公开面派生曝光，不缓存 exposed；`market()`、`codes()` 只读借用。公开库只在对象内部供曝光查询，没有引入无人使用的 getter。
- caller：两处 exposure 金样与两处 discovery failure 测试均直接消费 fixture 方法；as_of、个体 RNG、watchlist 与个人信息断言仍显式。
- 测试：原公布前空集合、公布后只有公告股票、私有事实不改变候选、曝光不自动获知全部断言保留。root 精确短筛选：`--test attention_discovery announcement_exposure_enters_weights_only_through_the_public_surface`；`--test attention_discovery discovery_sampling_never_writes_into_information_state`。
- 未完成：运行上述测试与独立复核。

## hosts-R2-N02

- 状态：源码完成，验证与独立复核待执行。
- 文件：`packages/engine/tests/auction.rs`。
- 所有权：`AuctionFixture` 拥有 SessionSetup 与 seed；证券身份来自其 owned setup；订单与昨日收盘继续由调用测试显式输入，运行 Session 与临时 SaveSlot 归各测试。
- 方法：`quiet(auction_ticks, seed)`、`representative(seed)`、`quiet_on_exchange(...)`、`retain_stock(code)`、`start_session()`、`save_with_orders(orders, previous_close)`、`restore_with_orders(...)`。注入方法直接同步行情、auction_orders、next_order_id、排序后的 live_envelopes，移除旧恢复流水线与 setup free function。
- caller：全部同文件测试迁移；单股 NPC 场景用 retain_stock，原 seed/config、沪深代码对应、价格单位与全部撮合/冻结/撤单断言保留。多股 pre-open 恢复也复用同一注入流水线。
- 测试：新增 `auction_fixture_preserves_exchange_identity_and_order_envelope_keys`；root 精确短筛选：`--test auction auction_fixture_preserves_exchange_identity_and_order_envelope_keys`；`--test auction shanghai_and_shenzhen_apply_their_own_final_auction_tie_breaks`。
- 未完成：运行上述测试与独立复核。

## hosts-R2-N04

- 状态：源码完成，验证与独立复核待执行。
- 文件：`packages/engine/tests/behavior.rs`。
- 所有权：`BehaviorScenario` 共同拥有 MarketView 与 BehaviorMarketObservation；StrategyData、SelfView、risk、RNG 与经历仍留在测试。
- 方法：`from_paths(paths, decline_fraction)` 从同一键集派生两项输入；`set_market_breadth(total, observed, equal_weight_return)` 一次更新原三项联动字段并保留构造时的涨跌比例；`market()`、`observations()` 只读借用。
- caller：原 39 个行为测试均直接借用场景方法进入原 engine API，两个市场宽度直接散改点改为具名 setter；calm/stress 场景各自独立。
- 测试：新增 `behavior_scenario_keeps_stock_paths_and_breadth_inputs_together` 检查代码集合、三个比例及 setter；原 108 处断言保持。root 精确短筛选：`--test behavior behavior_scenario_keeps_stock_paths_and_breadth_inputs_together`；`--test behavior unrelated_market_stocks_do_not_change_a_retailers_target`。
- 未完成：运行上述测试与独立复核。

## 已执行的静态核查

- 定向 `rustfmt --edition 2021` 成功，只处理以上 owned 源码文件。
- 以上文件的 `git diff --check` 成功。
- 旧 helper 残留搜索为空，所有原 test 函数仍在；未删除、重命名或弱化旧测试。
- 定向比对旧断言：auction 104、behavior 108、save_contract 主模块 22/失败模块 20、information_acquisition 三消费模块 21/16/13、exposure 9、discovery failures 8 均仅发生调用对象替换；weekend 24 处断言人工核对保持相同条件。
- 本批持有源码只消费 SaveSlot DTO、SelfView/PositionView 等视图，没有 Account/Position 生产对象字段需要迁移 getter；不修改同名 DTO 字段。
- 没有执行 Cargo，以上均不是运行时测试通过证据。

## Root 最终代表性验证（2026-10-03）

本组最终状态以 status.json 的 final_validation 为准。root build07 与 all-targets check08 均通过；root 选定精确 Rust case 的逐条结果已按 source 映射，WS 两个 bind sandbox EPERM 保留初始失败记录并由沙箱外原断言 2/2 通过核销。Writer 5/5 与桌面 CLI 5/5 通过。未选中的旧测试、完整 suite、API doctest（actors）及真实 matrix/E2E/性能未据此声称执行。历史“worker 未运行/待 root”段落保留为过程证据，当前没有未关闭实施或独立复核发现。
