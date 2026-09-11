# 任务 27 兼容性移除映射（compatibility-removal.md）

> 主题：`feat(engine): 固化公司与个体状态的新存档契约`
> 范围：Rust 权威引擎存档契约（SaveSlot/restore）。TS validator、旧
> `schema_version` 特判、TS fixture、WASM normalization 归任务 29/30，
> 本任务不提前要求旧 TS 类型适配未生成字段。

## 1. 删除的过渡期（legacy）专用路径

| # | 旧路径（任务 26 过渡边界） | 处置 | 依据 |
|---|---|---|---|
| 1 | `GameSession::restore` 前史重建 + 经营按自然日逐日重放（`while next_expected_date() < current_date` 循环调用 `advance_civil_day`） | **删除**：公司域权威状态（CompanyOperations：调度器/活跃冲击/各经营 RNG/账套）直接随档序列化，恢复整体覆盖 | K7「存档后演化入档」；issues.md 任务 26 §1 |
| 2 | `CompanyOperationsClockWiring::adopt_all_pending`（把当前全部待办视为已镜像的恢复专用收编） | **删除**：镜像集合（已镜像调度事件 id）本身入档；恢复边界改为 `mirror_is_exact` 精确校验（镜像 == 当前待办 id 集），失步 = 类型化拒绝 | 同上；task-14 复核 F-O3 语义保留（prune_dispatched 不变） |
| 3 | 恢复后复位信念/计划/信息集/关注列表（BeliefBook/PlanBook/NpcInformationState/PersonalWatchlist 不入档） | **删除**：四类个体状态全部必填入档；恢复后与不中断实例逐字节连续（save_contract 两枚等价测试 + session.rs 两测试回归原始断言强度） | issues.md 任务 26 §1 (a) |
| 4 | 恢复时剥离 `ParentOrderPlan.linked_plan_id`（防死引用） | **删除**：计划簿随档固化，链接关系保留；恢复边界改为互洽校验（链接计划必须存在、非终止、同账户同股票），违约 = 拒绝 | issues.md 任务 26 §1 |
| 5 | 恢复后第一次 `end_civil_day` 集中补发积压披露（游标复位所致） | **删除**：`DisclosureDispatch` 游标（published_through/announced_through）入档；恢复边界校验游标 == 已日结日 18:00 相位、库内无晚于游标的公布 | issues.md 任务 26 §1 (b)(c) |
| 6 | `CivilClock::from_parts` 按 `TradingCalendar::default_v1()` 重建日历（政策 v1 与本发布绑定的过渡假设） | **删除**：`CivilClockSave.policy: CalendarPolicySpec` 必填随档冻结；恢复走 `CalendarPolicy::from_parts`（内层 digest 先行复核）+ `TradingCalendar::from_policy`（外层 content digest 复核），绝不被当前进程默认表覆盖 | K1 冻结政策；civil_clock.rs 文档注释 |
| 7 | 旧 V 专用存档字段/测试（任务 26 已删的 v_params/fundamental_value_means/v_initial/Event::VError） | **维持删除**（任务 26 完成）；本任务无新增 V 路径 | issues.md 任务 26 §6 |

## 2. 旧格式存档的处置（无迁移器原则）

- **没有 legacy 分支、没有版本迁移器、没有 schema_version 字段。**
- 旧形状（缺任一 K7 必填字段：company_operations/closing_registry/public_library/ops_wiring/disclosures/plans/information_states/belief_books/watchlists/pending_plan_events，或 civil_clock 缺 policy、setup 缺 simulation_policy_id）→ serde 通用拒绝（missing field），由 `decode_save_slot` 统一包装为 `SessionError::InvalidSave`。
- 多余顶层字段（含旧 `schema_version`）→ `SaveSlot` 上的 `#[serde(deny_unknown_fields)]` 通用拒绝（既有行为，session.rs `current_save_json_requires_explicit_stock_fields` 原有断言维持）。
- 锁定测试：`tests/save_contract/failures.rs::missing_k7_fields_and_unknown_fields_are_generic_schema_rejections`。

## 3. 新增必填状态与恢复交叉校验（K7）

**入档（全部必填）**：CompanyOperations、ClosingEngine、PublicLibrary、
CompanyOperationsClockWiring（镜像）、DisclosureDispatch、PlanBook、
NpcInformationState 表、BeliefBook 表、PersonalWatchlist 表、
PendingPlanEvent 队列（存档边界只保留计划簿中仍存活的条目——未知/已终止
计划的迟到条目显式丢弃，永不适用；fix-round §3 定型）、CivilClockSave.policy
（冻结日历政策）、SessionSetup.simulation_policy_id（K7 行 174；规范值
`SIMULATION_POLICY_ID_V1 = "a-share-simulation-v1"`，恢复时不与「最新默认」
比对或迁移，身份不匹配由宿主层拒绝）。

**恢复边界交叉校验**（`validate_save_slot` 深化，全部类型化拒绝、零变更语义）：
公司集合精确覆盖 setup 股票且股本一致；经营 next_expected == 时钟当前日；
镜像 == 调度待办 id 集；时钟到期队列按 (日期,种类) 多重集**包含**调度待办
（时钟是通用注册表，第三方 register_due 合法共存；两边 id 是不同空间无法逐
id 连接）；PublicLibrary from_parts 全量重验；披露游标自洽 + 无未来公布；
三图（信念/信息集/关注列表）键集一致 + 与确定性重建的信念账户集合精确相等；
获知记录引用存在、observed_at ≥ published_at、无未来观察；信念条目引用的
报告 ⊆ 本人已获知；计划簿引用域/进度/链接母单互洽；pending 事件目标计划
存活、order id 在已分配域、trading_day 不超存档日。

**资源门禁**（可配置 `SaveDecodeLimits`）：JSON 总解码字节 ≤ 512 MiB、
公司数 ≤ 256（`MAX_SAVE_COMPANIES`）、计划簿 ≤ 1e6、公布/获知 ≤ 1e6、
pending 事件 ≤ 1e5，全部 `SessionError::ResourceLimit` 类型化拒绝，集合长度
用 checked_add 防溢出；任何失败原会话与源字节不变。

## 4. PersonalPriceMemory 现状核实

`src/experience/price_memory.rs`（任务 19 产物）**未被 GameSession 持有**
（session/decision_chain 无任何引用；技术观察走 `observation::build_technical_observation`
的公共历史），因此不属于存档契约范围——无状态可固化。待后续任务接线个人
价格记忆时按本契约同型入档（决策记录见 issues.md 任务 27 登记）。

## 5. 语义不变量（未弱化）

- 现有原子恢复/精度断言全部保留：session.rs 存档校验（价格历史/分钟收盘/
  T+1/冻结/预约）与 restore 深度校验原样；`restored_period_boundary_is_exactly_once`
  恰好一次语义维持（第三方自注册 dues 与经营 dues 合法共存）。
- extraction_replay 三锚重钉：events 不变（7_100_597_875_750_696_841，
  行为零漂移）；mid 18_072_312_056_192_250_746 → 13_844_125_012_886_974_023、
  end 5_864_974_982_281_894_531 → 5_750_120_563_743_802_542（仅存档形状新增
  K7 字段所致；同 seed 字节重放与区分力子测试结构不变）。
