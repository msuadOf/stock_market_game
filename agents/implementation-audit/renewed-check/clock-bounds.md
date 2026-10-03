# CivilClock 恢复日期适用边界复核

## 范围

- 基线：`.worktree/implementation-reaudit`，`HEAD=c0ab4299d104f07589008fee1af886198a2f783b`。
- 只静态追踪 `CivilClockSave` 到 `GameSession::restore` 的恢复链、时钟 owner 与完整存档校验，并对照 G15/G71/G75/G78/Q13 去重。
- 未运行测试或构造输入；未修改产品代码、总账或 Git 状态；未重新检索官方法源。

## 调用链与发现

`CivilClockSave` 是 `SaveSlot.civil_clock` 的持久字段（`packages/engine/src/session.rs:409-465`）。字节入口 `decode_save_slot` 负责大小/schema/serde 解码；`GameSession::restore` 先调用 `validate_save_slot`，再由 `CivilClock::from_parts` 恢复时钟（`session/persistence.rs:258`；`session.rs:2716-2720,2801-2805`）。

完整校验的 `validate_company_domain` 将 `civil_clock.pending_due` 按 `(due_date, DueKind)` 计数，用它与经营 scheduler 待办做包含匹配；它不检查 pending 日期是否落在冻结 `CalendarPolicy` 的适用范围（`session/persistence.rs:977-1045`）。随后 `CivilClock::from_parts` 重建并校验 policy、检查 `start_date` 是合法运行开局，并要求 pending ID 小于游标、pending 日期不早于 `current_date`；它没有对 pending 日期调用日历查询（`session/civil_clock.rs:238-294`）。因此完整 `SaveSlot` 恢复可以接受 `due_date > policy.runtime_max_end()` 的未来待办，只要其他恢复约束满足。

这与注册入口不一致：`register_due` 先拒绝早于当前日的日期，再调用 `calendar.day_status(exchange, due_date)?`；该查询强制日期位于日历适用区间，政策上界外会返回错误（`session/civil_clock.rs:330-343`；`calendar/holidays.rs:50-72`）。恢复数据因而能表达公开注册 API 无法建立的待办。此待办位于模拟推进上界之外，无法作为可运行的自然日到期事件；文档规定运行推进上界是 `2099-12-31`，越界显式错误（`docs/simulation-calendar.md` §2）。

日期关系需要限定准确：当前日期由恢复校验约束与 start/settled 状态相邻，且经营推进日期必须等于 `civil_clock.current_date`；`from_parts` 对 start date 运行范围做验证，而对每个 pending 日期仅做“不早于当前日”校验。这里的发现是**未来 pending due 日期**遗漏适用范围校验，不能把它表述成已证明完整恢复接受任意越界 `current_date`，也不把 `current_date` 的其他可能边界混入本条。待办日期等于当前日与晚于当前日都受相同的日历适用范围约束；完整校验不会因其对应 scheduler due 就补上这项约束。

## 去重裁定

- 这是同一 owner、同一 `CivilClockSave` 完整恢复入口的一项遗漏恢复不变量，建议扩展既有 G78 描述：除 pending ID 唯一与游标耗尽错误外，恢复还应拒绝适用范围外的 pending 日期，使恢复态符合 `register_due` 的日期准入规则。它不另建 G 编号。
- G15 是已存在官方年度 coverage 与模拟回退的优先级问题；本发现没有涉及假日计算或官方覆盖行为。G71 属另一 owner `OperatingScheduler` 的 `u64` 序号/身份命名空间。G75 是 `CivilInstant` 秒域反序列化校验。均不覆盖本项。
- Q13 讨论混合交易所/首所时钟策略，不涉及单一冻结日历政策中的 pending 日期越界；也没有据此主张变更沪深交易语义。
- 不以新局 `next_due_seq` 初值为 1 推断空队列游标 0 非法；不要求 `register_due` 或其他低层操作具备未承诺的全状态失败原子性。

## 领域依据与限制

该判断依据游戏自身 K1 约定：模拟日期运行上界及越界显式错误、存档冻结日历政策、外部存档须校验自身结构与不变量（`docs/simulation-calendar.md` §2、§3.4；`docs/principles.md` 原则 2）。公历 2000–2099 范围是文档记载的日期算法/游戏模型边界，不是交易所或中国结算发布的交易制度。未据本项声称任何新的 A 股法定日历规则，也未重新核验现行官方材料。

此记录是静态恢复边界发现，不是动态复现或测试结论。建议将该遗漏作为 G78 的范围补充交给总账维护者裁定。
