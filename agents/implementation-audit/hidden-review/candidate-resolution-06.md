# 隐藏候选裁定 06

## 范围与口径

- 对照基线为 `.worktree/implementation-reaudit` 的 `43b1aa5`。逐篇核对 `batch-160.md`、`batch-176.md`、`batch-191.md`，并对照总账 G01–G76/Q、候选裁定 01–05、完整 `SaveSlot` 恢复链及相关 owner/caller。
- 只做静态裁定；不运行测试、不构造利用输入、不改产品代码或总账。未重新核验交易所、中国结算或会计准则法源。
- `docs/principles.md` 允许编辑存档中的资产事实，但要求外部输入（含存档）校验自身结构与不变量；不能因状态通常由程序生成就免除恢复校验。另一方面，不将某一 caller 的 checkpoint 回滚扩张成低层方法普遍的失败原子契约。

## 逐项裁定

### CivilClock 重复身份与序号耗尽：确定独立缺口，建议登记 G78

`CivilClockSave` 是 `SaveSlot.civil_clock` 的持久字段（`packages/engine/src/session.rs:409-465`）。完整路径为 `decode_save_slot` 反序列化、`GameSession::restore` 调 `validate_save_slot`，随后在恢复过程中调用 `CivilClock::from_parts`（`session/persistence.rs:963-977,258-260`；`session.rs:2716-2718,2790`）。`validate_save_slot` 没有预先补足以下时钟不变量；`from_parts` 的现行检查（`session/civil_clock.rs:238-294`）也未检查：

- `pending_due` 中 `DueBusinessId` 唯一。检查仅要求每个 ID 小于游标并且日期不早于当前日；重复 ID 可留在恢复队列中。ID 文档将其定义为注册时分配、单调递增且不复用（`:57-76`），重复身份违反此序列身份不变量。
- 游标耗尽时显式拒绝。`register_due` 先以 `next_due_seq` 构造 ID，再执行裸 `+= 1`（`:330-356`）；`u32::MAX` 可由外部 `CivilClockSave` 注入且当前恢复检查接受，后续注册在溢出检查构建会 panic，在回绕构建会重复分配低 ID。应要求恢复校验并为分配耗尽提供显式错误；`u32::MAX` 是可分配 ID 还是保留终止游标，需由实现契约决定，不能把历史 sentinel 建议当成既定规则。

`next_due_seq == 0` 且队列为空也会通过现行 `from_parts`；不过 `CivilClock::new` 从 1 起始（`:221-230`）只是受控构造的初值，`DueBusinessId` 文档没有声明 0 保留或禁止，当前消费也只用于镜像集合、按 ID 排序和持久化，并未发现 0 与其他 ID 冲突。因此不把“新局从 1 起”提升为存档结构不变量；是否应拒绝该游标需先明确身份编号契约，当前仅列为恢复准入观察，不纳入 G78。

完整恢复不会替代重复身份与耗尽检查：后续 `validate_company_domain` 对时钟队列只按 `(due_date, DueKind)` 多重集核对 scheduler dues（`persistence.rs:1000-1047`），不检查时钟 ID 唯一或游标溢出安全。`CivilClock::from_parts` 为公开入口，完整存档恢复也实际调用它；因此重复身份与 MAX 耗尽是当前外部输入可达的恢复边界，不以“正常运行不容易注册到 MAX”排除。

与 G71 去重：G71 处理的是 `OperatingScheduler` 的 `u64` 调度 ID 唯一性/耗尽及 SaveSlot 恢复。此处是另一 owner `CivilClock` 的 `u32 DueBusinessId` 命名空间及其真实消费者，不是 scheduler ID 的重复描述，应单独登记 G78。它不改变到期业务种类、自然日规则或 A 股交易语义。

### CompanyOperationsClockWiring.sync 部分写入：确认状态风险；不新增 G/Q

`sync` 对每个待办先执行 `self.mirrored.insert(due.id.value())`，再调用可能返回错误的 `clock.register_due`（`session/company_operations.rs:61-74`）。若注册失败，该 ID 已被标记；再次调用会跳过它并可能返回 `Ok(0)`，clock 仍缺对应 due。多条待办也可能在后续失败前留下部分注册和镜像。

生产入口有两类：`GameSession::new` 通过 `install` 初始化镜像（`session.rs:1403-1408`）；`end_civil_day` 通过 `run_day_end` 同步，且整个日终外围 checkpoint 在失败时恢复整个 session（`:2090-2122`）。初始化失败时 session 不返回，日终失败时会回滚外层候选。当前低层 `sync`/`install` 没有写明“Err 时自身状态零变更”的承诺；不能把日终 caller 的 checkpoint 扩大成 adapter 通用强原子契约。静态材料也未证明合法新局或通过完整存档校验的生产待办能使这里的注册失败。因此记录为真实的错误后重试风险，但本轮不据此升为现行 G/Q；若后续要要求可重试同步，应先明确 `sync` 失败后的状态/重试契约及其受支持 caller，再按该边界验证，不要求底层任意相关状态全事务化。

### IndustrialBooks.available_credit 错误折叠：确定独立候选，建议登记 G79

公开方法的注释定义 `None` 为“无授信”，实现却用 `self.loans.outstanding_total().ok()?` 和 `limit.sub(outstanding).ok()` 把两种金额计算错误也折叠成 `None`（`company/industrial/mod.rs:178-182`）。这不是单纯的理论隐患：`IndustrialBooks`、`LoanState` 与贷款组合均直接派生 `Deserialize`（`industrial/mod.rs:57-72`；`industrial/loans.rs:17-23,79-101`），工业状态位于 serde 持久化的 `CompanyOperations` / `SaveSlot`（`company/operations/core.rs:127-140`；`session.rs:465`）。`GameSession::restore` 校验公司映射和 scheduler/clock，但没有深校验工业贷款本金范围或总额可加性（`session/persistence.rs:976-1049`；恢复后直接装入 `save.company_operations`，`session.rs:2955-2970`）。可编辑存档因此能提供使总未偿额加总溢出的输入；直接公开 `IndustrialBooks` 反序列化也没有专用校验入口。这里无需假定正常借款流程会生成该状态。

`available_credit` 只有在确实没有该 lender 的授信额度时，按其文档约定返回 `None`；金额加总或减法失败应可见地报告，而非伪装成无授信。G58 是多个 lender 共用全部 outstanding 导致的授信归属计算错误，不覆盖计算溢出被吞并；G73 是保险合同组恢复不变量，也不覆盖工业贷款状态。故这是不同方法/状态不变量的独立候选，建议登记 G79。结论限于输入校验和公开 API 错误表达，不推导会计准则或 A 股交易规则变化。

### issuer mapping 重复 tuple：完整产品入口已拒绝；不新增 G/Q

`CompanyRegistry::validate_issuer_mapping` 对 `stocks` 使用首个同代码 tuple 查股本（`company/mod.rs:222-248`），单独传入重复代码且不同股本的 slice 时不会完整验证该 slice 的唯一性。但产品组装 caller 先经过 `setup.validate()`，再构造 tuple 列表并调用映射校验（`session/company_assembly.rs:68-88`）；`GameConfig::validate` 及 Setup 校验为证券列表提供唯一代码约束（`config.rs:132`，setup 校验入口在 `session.rs:951`）。发行人重复由 `CompanySpec::validate_set` 拒绝（`company/spec.rs:94-126`）。因此指定的完整新局入口先排除了重复股票 tuple，当前证据不足以把该直接 helper 的宽输入行为提升为生产缺口；不并入其他 G，也不另建 G/Q。若将来把 helper slice 明确承诺为独立外部输入，则可单独重审其参数唯一性契约。

## 去重与限制

- G70 已覆盖工业开局库存 seed 与总账对账；G73 已覆盖保险组外部恢复状态校验。本裁定不重记两项，也不把它们扩展成一般性的“所有 company operation 全事务”要求。
- 新增候选仅为 G78（CivilClock 重复身份/耗尽恢复）和 G79（available_credit 错误折叠及外部状态校验）。空队列零游标单列为待明确契约的恢复准入观察，不计入 G78。G 编号为建议编号，正式并入仍由总账维护者执行。
- `sync` 的镜像先写风险与上述恢复/API 校验不同；由于没有低层失败零变更契约，且生产日终 caller 有外层 checkpoint，本裁定不新增项。外层回滚只作为 caller 证据，不证明 `sync` 自身原子。
- 无测试、构建、动态复现或官方规则检索。本文是静态候选裁定，不构成行为验收或交易规则复核。
