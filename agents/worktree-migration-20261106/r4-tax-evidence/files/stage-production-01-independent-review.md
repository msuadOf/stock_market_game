# stage-production-01 独立源码审查

日期：2026-10-29。裁决：**APPROVED_SOURCE_ADMISSION**。

本审未参与本批实现，全程只读完成源码审查，随后仅归档本报告；未修改产品、未运行 Cargo。该裁决允许冻结片进入实际构建与运行验证，不表示实际 green 或生产功能完成。

## 冻结范围与绑定

审查树：`/data1/<USER>/workplace/stock_market_game/.worktree/r4-tax-production-2026-10-29`。

基线：`e78cb9e81da7070afccd4c43bb59e796066e8185`。

冻结原件为同目录的 [stage-production-01.patch](stage-production-01.patch) 与 [stage-production-01-fingerprints.json](stage-production-01-fingerprints.json)。本审实际核对八份文件的 SHA-256，全部与 fingerprints 一致；六份 tracked diff 加两份新增源码构成的完整 live diff 与冻结 patch 逐字节一致。`git diff --check` 无输出。

patch SHA-256：`c52b3e76fb819eec20c1975dc0dc3950e99cff3066ae691fb87236764b9f481c`。

以下路径均相对审查树根目录：

| 文件 | SHA-256 |
|------|---------|
| `packages/engine/src/runtime/protocol.rs` | `a5e874e8e5283f356ac27a095ae1058950bf95ad60edcfdd7245607ddfe727ff` |
| `packages/engine/src/runtime/group.rs` | `1b9c11db047b660aae72434ac8dee3cdc5211d57d960cc0be8ff870ed87acc61` |
| `packages/engine/src/runtime/group/admin_transaction.rs` | `d3621ed8b5cf7361508ec6783f8b9227356d843a2cc44dc3984d61e6f168e594` |
| `packages/engine/src/runtime/world/admin_transaction.rs` | `697db02caadcc39dae2e794a5a7ddaaf88978e82b012a23bd7580177f76337b4` |
| `packages/engine/src/runtime/world/admin_transaction/stages.rs` | `a38460064cb717a814a90cc6a9302ad7144cc7cf63842aa909f8046b2add374b` |
| `packages/engine/src/runtime/world/admin_transaction/stage_tests.rs` | `45dcd607dab65c90e5cab478da65bf5bd439489aad4ec0b7cd1e9fd45f746030` |
| `packages/engine/src/runtime/group/admin_transaction/stages.rs` | `3987edd2ff20b3bb305817e7c3c4adf3348988fd022549d548e9adb88e688029` |
| `packages/engine/src/runtime/world/admin_transaction/stage_information_tests.rs` | `f14929846f8f270c13fa8362dd606506a1445912f5e04a9549189aa7a148169b` |

本报告只签上述冻结八文件。integration 后续在 `group.rs` 合成的其他 fixture、其他 patch 或后续改动不在此绑定中，须由其完整合成独审覆盖。

## 唯一有效发现与修复复核

**F1：税阶段实际账户来源损坏曾被错误归类为普通业务拒绝；已修复，源码复核关闭。**

具体输入为合法的 `CashDividend 40 → WithholdingTax 25/5 → PaymentDebit 10`，现金为 `100→140→110→100`。若组端 Prepare 回执的税阶段 `after.cash` 被损坏为 `109`，但 stage、operation indices 与 local 集保持合法，原实现的结构校验会放行；随后 `prepare_stage_source → inspect_tax_stage` 在用户 inspect 包装闭包内发现完整账户不符，错误被 `execute_admin` 当作普通 inspect 业务 Err，Abort 后 World 仍可复用。这违反来源协议损坏须 terminal 的已决合同。

冻结修复在 `world/admin_transaction/stages.rs::validate_group_stage_evidence` 接收真实事务 id，匹配 stage、indices 和完整税 local 集后，以该 stage 的原 flat operand 引用和真实 before/after 调用既有 `inspect_tax_stage` core。该校验由 `world/admin_transaction.rs::execute_admin` 在协议回执验证路径调用，失败通过现有 `AdminFailure::from` 进入 terminal 收尾。最终 inspect 仍借原 typed operand 构造完整投影；没有复制税算术或新增通用 callback 框架。用户 inspect 主动拒绝仍沿原普通 Abort 分支，保留原始原因。

新增 stage04 的 `StagePrepareCorruption(3)` 只令真实税阶段 `after.cash -= 1`，保留合法 indices/local，并断言用户 inspect 未调用、`dead_reason` 存在、全部 worker handles 已回收。其余五种注入覆盖操作索引缺失、重复、重排、税阶段重复和税 local 错位。正控以指针相等确认 inspect 借用 World 保留的原 typed 请求。上述测试已完成源码核对，尚未实际运行。

## AGENTS 三问

1. **是否符合大 A 语义，依据是否可靠：通过本片限定。** 本审读取已决 stage v3 合同及其独审、`central-next-tests.md`、ADR-0040 与 `docs/trading-rules.md` 的现有行政数值事务边界。本片沿已审的 `CashDividend`、`WithholdingTax`、`PaymentDebit` 原语，金额为整数分；只借记已确定的正税额，不新增税率、税日、法定付款顺序或机构税制。本审没有重新访问官方网页，不据此签发新税法解释。Prepared 操作与税投影仍是可 Abort 的候选证据，没有被冒充已提交实收、`Paid`、`Settled` 或 `Credited`。
2. **是否为需求所必需、是否保持最小范围：通过。** staged 与旧入口共用一次 `with_administrative_inspection`、唯一 pending/id、Prepare、inspect、Commit/Abort。旧 flat 入口使用 `None` 薄适配，同一 `prepare_working` 保留原 checked apply 顺序；repurchase 仍先按原 flat 位置解析 `CreatedInThisPrepare`，没有按 stage 归零。原 typed operands 继续是唯一请求内容，新增 counts、成功 apply 后记录的索引和税阶段 touched locals 的 before/after；未增加第二账户权威、持久执行账或另一套税算法。INFO 仍只在末尾调用一次现有 core。
3. **是否遗漏边界测试、存在跨层语义漂移或不必要复杂度：限定源码准入通过，运行及更广边界仍待后续。** 未发现剩余阻断源码问题。World 空调用仍 inspect 一次；INFO Some 仍补齐全部 live groups，允许空 operations；税与非税混合 stage、重复或越界 group 在下发前显式拒绝。缺失、重复、重排的回执与税账户来源损坏进入协议 fatal；尾扣不足及用户 inspect 拒绝仍全事务 Abort。新增测试没有弱化原断言，且来源校验继续复用唯一税 core。尚未取得本冻结片的实际构建或 green，不得将源码准入作为验收完成。

## 已核首红与回归保护的证据边界

本审实际读取 `.worktree/d3-p1-integration-2026-10-28/agents/branch-completion/joint-43-2026-10-29/` 下三份原始 stderr：

| case | 实际失败位置与原因 |
|------|--------------------|
| `r4_stage_01_credit_tax_debit_uses_real_working_stage_accounts` | `stage_tests.rs:106`，成功 expect 收到 placeholder 的 `R4 ordered stages 的同 pending 实际来源尚未接通` |
| `r4_stage_02_late_debit_rejection_aborts_credit_and_tax` | `stage_tests.rs:135`，真实扣款原因断言收到同一 placeholder 错误 |
| `r4_stage_03_final_inspect_rejection_aborts_tax_and_keeps_original_cause` | `stage_tests.rs:163`，原始 inspect 原因断言收到同一 placeholder 错误 |

三份原件文件名分别为 `feature-engine-<上述case>.stderr`。三门是既有真实首红，本审未重跑。

stage04 与 stage05 是实现后新增的回归保护，尚无实际 red/green，不能回填为原先 TDD 首红。stage05 的真实三组 World、Calendar、五名个人与构造期能力名册已核；它比较两种拒绝后的完整账户、计划、完整个人态（含 RNG）、World source 和 pending domains，并以相同 INFO payload 合法重试，核最终候选及 Commit 后的公告观察。该证据保护上述五类读面与重试行为，**不等于逐字段验证全部 Group 私域的 library、current facts、水位、signature 和 dirty 状态**。

当前 `with_staged_admin_transaction` 只有 World 定义与短测试调用，没有 Session/distributions 资金生产 caller。staged 入口仍沿 legacy 丢弃未消费的 mirror ack；与真实 `PreparedAdministrativeSource`、Session 固定成功尾安装、公司 finance/registry/税账勾稽的接线须后续验证。本片不签真实分红付款、collection、`Paid`、双 INFO 观察点、下一真实 Decision 消费、save/Protocol/observer 发布或完整公司行为链已完成。

**最终结论：冻结八文件可进入实际验证；F1 已源码修复并独立复核，暂无剩余阻断源码发现。无实际 green，无完整生产交付签过。**

## 追加：stage05 E0282 最小编译修正窄复核

日期：2026-10-29。裁决：**APPROVED_SOURCE_ADMISSION（仅本节一行测试类型修正）**。上文冻结文件、SHA、patch 与历史审查结论原样保留，本节不重新签 integration 的合成 diff。

父级转交的 integration50 运行记录为：session `35557`、`WAIT1`、chunk `3ea2a4`，wall `59.460s`，实际 compile FAIL、执行 case 数为 `0`；报告的唯一错误是 `stage_information_tests.rs:116` 的 E0282。原闭包只执行 `panic!`，没有可推断的成功值，泛型返回值 `P` 无法确定。本次实际编译暴露了原源码审查未发现的类型推断问题；不能将该失败称为业务首红或任何 case 的运行结果。**本审未亲读该次原始 stderr，也未执行构建；这些运行数据按父级转交记录归档，不扩充为本审独立运行证据。**

本审从原 `stage-production-01.patch` 提取冻结新文件，核得原 SHA-256 为 `f14929846f8f270c13fa8362dd606506a1445912f5e04a9549189aa7a148169b`；与当前文件逐行比较，完整净差量仅为一行替换：

```diff
-        .with_staged_admin_transaction(stages(111), Some(information.clone()), |_, _| {
+        .with_staged_admin_transaction(stages(111), Some(information.clone()), |_, _| -> Result<(), RuntimeError> {
```

当前唯一修改文件为 `packages/engine/src/runtime/world/admin_transaction/stage_information_tests.rs`，SHA-256 已实际核为 `f4163ef996a53732bbafa1e82f166f16171f865d60923fa5da8da6895ee5fd1e`。

`RuntimeError` 的来源已顺源码核对：`runtime/world.rs` 从 `runtime::protocol` 引入该类型，`admin_transaction`、`stages` 和本测试模块逐层通过 `use super::*` 使用同一个类型。`with_staged_admin_transaction` 的 inspect 参数要求 `Result<P, RuntimeError>`；显式注解将此闭包的 `P` 确定为 `()`，`panic!` 保留原遇到错误 inspect 调用就失败的行为。未改 panic 内容、输入、任何断言、后续合法重试或产品代码。

AGENTS 三问的窄结论：本改动不触及 A 股领域行为；修正实际编译阻断所需的类型信息，范围最小；没有弱化测试或引入跨层语义变化。本审仅修改本报告，未碰产品、未运行 Cargo。修复后实际编译及 green 仍待 integration 取得，Session 付款等上文未签范围继续保留。
