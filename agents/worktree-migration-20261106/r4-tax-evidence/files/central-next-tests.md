# 后继 mixed 税阶段与真实现金接线的最窄门

日期：2026-10-29。状态：只读还原后的 proposal，尚未修改中央产品或写下列测试，不以拟议接口冒实物。

## 已决合同与实际缺口

原件位于 `.worktree/company-r4-production/agents/branch-completion/r4-production/`：`stage-contract-v3-2026-10-27.md`（独审冻结SHA `37236fb63e412bf077dab065c214741b856e4b1b3f21040b4b3a41d2b9113f91`）、`independent-review-stage-v3-2026-10-27.md`、`tax-stage-view-v1-2026-10-27.md`、`stage-production-tests-first-cd6-2026-10-28.md`。三份实际 producer 均未实现：

- `runtime/protocol.rs` 的 `InformationPreparePayload` 只含一个 observation，`AdminGroupCandidate` 仅最终 accounts，`AdminPrepare` 仍flat operations。不能核混合税阶段的真实 intermediate before/after。
- `runtime/group/admin_transaction.rs::prepare_admin_scope_with_clock` 一份working按flat ops顺序应用，随后一次INFO；可沿此working扩最窄ordered纯资产stage来源，不循环旧prepare、不多造pending/id。`runtime/world/admin_transaction.rs::with_administrative_inspection` 的一次prepare/inspect/commit/abort生命周期必须继续复用。
- `session/civil_information.rs` 向同次Admin发送空ops；`corporate_actions/distributions_day_end.rs` 的真实gross/withholding/collection仅候选，无生产caller。因此ordinary capture到Registered不代表可发钱、扣税或变Paid。

`admin_tax_projection`五门正式绿只证明tax-only整事务与唯一数值core；旧`admin_stage_evidence`11门只是未注册借用索引checker。两者都不提供mixed producer。

## 第一必要真实 World 门（建议同一 test module）

复用实际 `runtime/admin_tax_projection_tests.rs` 的三组/两股/两槽短World与完整GroupAccount快照，账户现金由构造事实100分；不手造AdminCandidate/id。新增最小可编译stage入口placeholder取业务红后才实施：

1. `r4_stage_01_credit_tax_debit_uses_real_working_stage_accounts`：同组非零local，stage0 CashDividend40，stage1 WithholdingTax25（股1）/5（股0），stage2 PaymentDebit10。最终cash100，但必须借同pending/id实际tax before140→after110，flat operation_index=1/2、stage_index=1；唯一inspect与finalCommit后才交prepared tax projection。完整GroupAccount除cash外保持相等。
2. `r4_stage_02_late_debit_rejection_aborts_credit_and_tax`：尾扣111>110，真实checked apply拒绝；inspect未调用、全部组完整GroupAccount不变，World健康，修正扣10后可成功。首三门没有INFO或计划变化，不能据此证明计划/个人/库/facts/水位的全域回滚；该边界留实际INFO/计划接缝后继门。不得拿第一阶段已commit再“补偿”倒账。
3. `r4_stage_03_final_inspect_rejection_aborts_tax_and_keeps_original_cause`：三stage均prepared后inspect返回独特typed原因；全组Abort同id，原原因保留、无税receipt/observer/save发布；合法后继可成功。

上述是纯资产ordered-stage+最终INFO接缝的最小代表切片，不能据此宣称两个真实INFO观察点已实现。旧空World调用、单stage与INFO入口必须作为薄适配保行为，repurchase CreatedInThisPrepare的group内flat index不能因stage归零。税before/after只复制该stage实际WithholdingTax touched locals；空组不伪造税view，混有其他typed op的税stage明确拒绝。

## 下一 Session 资金门

沿Tax03合法17月prehistory且真实strict restore fixture，AShareIndividual登记日完成后推进实际payable日：按真实名册gross逐账户CashDividend，再独立WithholdingTax；公司pay_dividend/个人record_payment/collect_due仅准备同一公司候选，并在真实组回执精确勾稽+唯一Commit后固定尾安装。正控逐账户真实cash与gross、公司应付清偿、税所得激活必须同时一致；拒绝/后组失败时不安装Paid/collection。AShareIndividual未卖出时税collection可为0，不造零额typed WithholdingTax；有真实既存应税处置及不足cash时，沿既有partial collection契约，不借buy purse或补资金。

税阶段顺序只沿R2 candidate实际意图及既有日终合同，不按kind猜法定时点。个人BonusShares、单位movement、来源未证的限售与R/R+1税日门保持；本轮不扩真实新规则或Simulation资金链。


首三门源码已冻结为 `stage-three-red-source-01.patch` / fingerprints，未编译/未运行。未参与实现 `gpt-6.1-sol` / `high` source reviewer 已核3个fixture无提前错误及真实扣款错误Display；同时明确两个后继缺口：当前测试只比较完整GroupAccount和健康续行，未比较计划/个人/库/facts/水位；placeholder的prepared结构没有原sent typed operand借用访问面，真实生产时必须接只读原引用join，不能用索引自报或复制请求充作来源。root已批准先保首三门tests-only目标红，不将上述缺口夸称闭合。
