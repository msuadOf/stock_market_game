# Save 与经营事件测试迁移独立复核

## 范围

复核 `packages/engine/tests/save_contract/main.rs`、`packages/engine/tests/save_contract/failures.rs`、`packages/engine/tests/company_event_contract.rs`、`packages/engine/tests/scale_restore_limits.rs` 相对 HEAD 的完整 diff，并检查相邻 `CompanyOperations` 测试是否保留被迁移的经营行为断言。未改代码或 Cargo/index 文件。

## 初审结论（已由增量 diff 复核）

- 将 `Simple` Session 的存档契约改为检查 `company_system` 和 `advanced_through`，符合当前 ADR-0035 所述边界；没有把旧 `CompanyOperations` 状态挂入 Simple Session。
- `scale_restore_limits` 把 collection 规模/股票映射校验 fixture 改成 Simple 公司系统，仍覆盖 257 家结构可解码、重复发行人股票冲突和 Session 股票配置映射不一致；该测试本身未见相关断言弱化。
- `company_event_contract` 的其余通用计划报告测试仍通过 `GameSession` 检查 publication event、公开查询结果和 date transition 顺序。冲击专属旧测试完整保存在 `agents/company-system/reference/session-shock-announcement-legacy.rs.txt`，其 Session 持有 `CompanyOperations` 的前置条件与当前 `Simple` Session 不符；作为未来真实经营仿真的门禁/参考保留是合理的，本次不要求给 Simple 加兼容影子状态。
- `save_contract/main.rs` 删除旧 scheduler `pending()`、due 日期与 `next_expected_date` 断言。初审时发现没有 save/restore 等价覆盖；增量 diff 已新增 `operating_scheduler_state_roundtrips_in_its_own_company_operations_fixture`：两日推进后核对 pending 非空、due 不早于下一日、预期日等于再下一日，并序列化恢复后核对 pending 和预期日。因此原 findings 已关闭。该测试仅直接序列化 `CompanyOperations`，不是 Session/SaveSlot 场景；若旧断言的目标仅是经营状态恢复则充分，若要证明该状态属于当前 Session 存档则不能这么解释，而本次契约明确不再如此承诺。
- 新公告 fixture 直接设 `occurred_on = 2030-01-01`，CivilClock 对同一日期 `end_day` 产生 disclosure instant；该测试断言 occurrence date 与 publication instant，未发现把 `advanced_through` 错当 publication date 的来源/时间错误。它目前只覆盖该日同日公告，不覆盖非交易日/延期发布时间语义。
- `failures.rs` 的 missing-field 检查从 `company_operations` 改为 `company_system`，符合 Session 格式变更；旧 `CompanyOperations` 自身恢复校验仍由相邻低层测试负责。当前 `bank_policy.rs` 保留独立经营状态的 ECL 输入校验，不涉及 Simple 后台兼容。

## 验证限制

未运行测试。owner 记录的定向 `cargo check` 只能说明目标测试可编译，不能证明新增动态断言通过；应由 root 计划中的统一 whole-check / 动态验收给出结果。当前没有未解决的范围内断言弱化；旧冲击专属 Session 事件测试保留为未来 Simulation 参考，不计作当前 Simple 契约缺陷。
