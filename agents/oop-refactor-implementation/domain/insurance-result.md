# insurance 实施结果

- 基线：`b89afb3346743a4b4fccf26c9ac9ff108595f696`。
- 权威动作正文：`agents/oop-refactor-audit/challenge-2026-10-03/action-index.md` 的 `domain-N04`（insurance 部分）与 `domain-R2-N18`。
- 代码写口仅为 `packages/engine/src/company/insurance/**`；未修改其他行业、会计底座、投资者交易、存档版本、Git 状态或正式文档。
- 已全文阅读 `AGENTS.md`、`docs/principles.md`、`docs/testing.md`、`docs/architecture.md`、`docs/open-questions.md`、ADR-0002、ADR-0016、`docs/company-accounting.md` 与动作正文/源码。无新增依赖，无开放技术问题决策。

## 逐动作落点

| 动作 | 文件 / owner / 方法 | 真实 caller | 保留的行为 |
|---|---|---|---|
| domain-N04（insurance） | `config.rs::InsuranceConfig::check_opening_lines(&self)` | `mod.rs::InsuranceBooks::new` | `DiscountAssumption::validate` → 六科目禁止清单按输入顺序取首个 → `Books::post_batch` → 对手方登记；不改为允许清单 |
| domain-R2-N18（计量/进度） | `groups.rs::ContractMeasurementState` 持有五个 GMM 余额、coverage 日期、units、五类 `FractionUnits` 与调节累计；`preview_release`、`preview_remeasure`、`apply_release`、`apply_remeasure` | `service_release.rs::InsuranceBooks::release_service`、`remeasure.rs::InsuranceBooks::remeasure` 在 post 前只预览，post 后调用 owner apply | 原 `unit_release`、`simple_discount_pv` 调用顺序与裸/checked 运算、CSM/亏损三向分流、末批尾差、零变动事件槽均保留；复用 `ReleaseBatch`，只新增临时 `RemeasureDelta` |
| domain-R2-N18（赔案账） | `claims.rs::ClaimRegister::{new,from_claims,get,iter,insert,apply_payment}` 唯一持有赔案映射；`ClaimState` 保留发生/支付/未付额行为 | `ContractGroupState::{claim,claims}` 查询；`InsuranceBooks::{record_claim,pay_claim}` 在 post 后直接要求 register 更新 | 原组/赔案/金额校验顺序、`Option<Result>`、累计溢出、对手方 flow 时点均保留 |
| domain-R2-N18（存档/聚合） | `groups.rs::ContractGroupState` 保留身份/保费字段并组合 measurement + register；`ContractGroupSnapshot` 专用扁平 serde 映射 | `premium.rs::InsuranceBooks::establish_group` 的既有 `ContractGroupState::new` 自动构造子对象；所有既有查询消费者无需改动；`InsuranceBooks` 存读档调用原 serde 接口 | 原 24 个字段及字段顺序、金额/carry 单位、必填字段、重复字段拒绝、未知字段忽略、sequence 输入接受均保留；Serialize 借用赔案映射，不克隆大账 |

`premium.rs` 不需要源代码变更：它继续调用同一组构造器与保费收款方法，构造器内部已创建新的唯一 owner。`operations/insurance.rs::advance_day` 仍通过 `InsuranceBooks` 建组、收保费、每日释放与发生/支付赔案；没有增加旁路流程。

## 领域语义与失败边界

- CAS 25 依据及适用日期沿用 `docs/company-accounting.md §2.4` 的财政部财会〔2020〕20号登记（其他执行企业会计准则的企业自 2026-01-01；默认 2030 开局）；GMM 五项游戏简化均未修改。此次不涉及交易制度，无新的交易所规则判断。
- 保费不是即期收入；赔案发生与现金支付分离；亏损成分仍为备查项，不加总到 LRC；LRC/LIC、`BusinessKind`、`CashFlowClass`、source id 槽位及投资者/公司资金隔离保持原样。
- post 失败不写子账；apply 阶段本来可以在极值存档下溢出并保留部分写入，本轮按原语句顺序迁移，**没有宣称全部 Err 原子**。释放累计溢出时五余额先递减而 units/carry 未推进；重估累计溢出时剩余预期/CSM/亏损先更新而后续累计未更新。
- 既有末批口径：有余额组件经 `unit_release` 清零；非正 CSM/亏损分支跳过 `unit_release` 并保留失活 carry。正常部分释放后重估耗尽 CSM 可留下旧 carry，本轮不借重构修复此领域行为，新增测试明确保留。

## 短行为测试与验证

新增 `behavior_tests.rs` 共 10 个短 case，无额外 features；集中 runner filter 为 `company::insurance::behavior_tests`（`--lib`）。

1. `opening_guard_keeps_discount_precedence_and_first_seeded_account`：Discount 错误优先与首个禁止科目。
2. `multiple_partial_payments_exhaust_claim_then_reject_one_cent`：多次部分支付累计归零，再付一分精确拒绝且序列化状态不变。
3. `flat_group_snapshot_keeps_required_duplicate_and_unknown_field_behavior`：24 字段、逐字段必填、sequence、未知字段接受、重复字段拒绝。
4. `restored_remeasurement_and_release_keep_all_tail_components_and_carries`：存档恢复后重估/分批释放一致、逐次 LRC 恒等式与末批余额/carry 清零。
5. `zero_remeasurement_consumes_one_event_slot_without_group_writes`：零变动占一槽，不写组状态。
6. `release_apply_overflow_preserves_existing_post_and_partial_write_order`：post 已提交后 release apply 极值溢出的原部分写入顺序。
7. `measurement_previews_are_pure_and_keep_exact_release_and_remeasurement_gold`：owner 纯预览、不提前写入、释放五分量/五余数与重估差额的手算金样。
8. `failed_post_leaves_measurement_unchanged_for_release_and_remeasurement`：重复 source 强制 post 失败，完整序列化状态不变。
9. `exhausted_csm_keeps_existing_inactive_carry_after_final_release`：重估耗尽 CSM 后末批保留失活 carry。
10. `remeasurement_apply_overflow_preserves_existing_partial_write_order`：remeasure apply 极值累计溢出的原部分写入顺序。

已有对应场景：`tests/insurance_accounting/{gold,remeasure,claims}.rs`、`failures/{entities,guards}.rs`；此次未修改既有断言。

实施顺序是先写现有 API 行为保护测试，再静态核对，再迁移 owner，最后增加 owner 纯预览保护。父协调者要求 worker 不因集中测试等待阻塞；基线测试未在实施前执行，未声称红绿循环。此 agent 遵守禁令，**未运行 Cargo、产品测试或完整回归**；由父协调者集中编译/执行并补充结果。定向 `rustfmt --config skip_children=true` 与 `git diff --check -- packages/engine/src/company/insurance` 已通过；不把静态检查称为行为验证。

## 最终门禁

root 最终冻结后编译与 10 个指定短 case 已通过；未参与实现的 reviewer 独立完整静态审查已完成，见 [insurance-review.md](insurance-review.md)。实际编译/运行证据见 [final-summary.md](final-summary.md)。本组没有待修有效发现或未完成事项，未运行完整回归。
