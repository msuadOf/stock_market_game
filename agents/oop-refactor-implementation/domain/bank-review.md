# bank 独立复核

日期：2026-10-03。canonical reviewer：`/root/implement_domain/review_bank`；审查者未实施此批改动；基线为 `b89afb3346743a4b4fccf26c9ac9ff108595f696`。最终身份补录时再次核对以下 9 个源码文件 SHA-256，全部与本记录的审查快照一致。

## 范围与结论

完整阅读基线到工作区的 bank diff：`config.rs`、`deposits.rs`、`ecl.rs`、`interest.rs`、`lending.rs`、`loans.rs`、`mod.rs`、`writeoff.rs`，并完整阅读未跟踪的 `behavior_tests.rs`。同时核对当前 bank 源码、既有 bank failure tests、相关生产 caller，以及权威 `challenge-2026-10-03/action-index.md` 的 domain-N04 银行部分、domain-R2-N10/N11/N12。

**未发现本批引入的阻断性 finding。** 这是静态审查结论；本审查未执行 cargo、测试、编译，也未取得 worker 的最终执行记录，不能据此声称测试通过或整个 domain 批次完成。

## 三门核对

1. **大 A 语义及依据：** 本批是银行公司经营子账的对象归属调整；未触碰证券投资者 cash/shares、撮合、沪深板块、T+1 或申报单位。客户存款仍是 Liability，贷款发放仍是 Asset；科目、PostingSide、CashFlowClass 均与基线相同。AccountingAmount 仍为分，rate/weight/PD/LGD 仍为 bp，FractionUnits 仍为 1/3_650_000 分。政策背景核对 ADR-0016、`docs/company-accounting.md` §2.3/§4/§6 和 `policy-sources.json` 中 `cas-22-financial-instruments-2017`：财政部财会〔2017〕7号，2017-03-31 发布，境内上市适用日 2019-01-01，原取证日期 2026-09-10，官方通知及 PDF URL 已登记。本次沿用这份已登记依据，未重新联网核验准则全文，也不新增制度判断。定期存款提前提取、到期停息、贷款逾期原利率/单利、ECL 不折现等原有简化保持不变；不将它们称为完整银行实务。
2. **必要性和最小范围：** BankConfig 只收回自己的禁止科目检查，未把禁止清单改为允许清单。DepositState/BankLoanState 收回单合同 guard 和 preview；BankBooks 继续拥有 map、JournalEntry、post、event id 和 counterparty flow。初始化 allowance 进入原 state constructor，消除构造后直接字段赋值；三个字段 private 化未新增持久化事实。LoanWriteOffSnapshot 仅有分录金额快照，EclScenariosRef 仅借用原切片；均不建立第二份状态。无新增依赖、备用实现或跨层 owner。各动作处于其已授权范围，未发现顺带功能变更。
3. **边界、跨层和复杂度：** 对照基线，unknown contract → 单合同 guard → post → apply → flow 的顺序保留；assess 的情景验证先于 unknown loan/核销阶段检查。Deposit preview 的有效截止仍是 min(through,maturity)，错误仍报告原 through；贷款 Stage1/2 本金基数、Stage3 非负净额基数、同日 skip、零分日期/余数推进及合同稳定顺序均保留。新增测试覆盖首错、部分提取余数、提款 PaymentFailed 完整不变、多情景累计后单次 half-even、ECL 乘法溢出、恢复后空初始情景、核销后计息与 recovery 部分 apply 顺序。未发现跨层概念或单位漂移。

## 特别复核项

- **post/apply 部分失败：** `interest.rs` 仍一次 post 后按 item 顺序 apply；`loans.rs::apply_accrual` 和 `deposits.rs::apply_accrual` 仍先 checked add，再更新 carried/date。`writeoff.rs::recover_written_off` 仍先提交总账及 event id，再 `apply_recovery`；后者仍先扣 recoverable，再加 allowance，最后才由 handler 记录 flow。因此 serde 可接受的极值 state 可能产生旧有部分提交；本批没有把失败移到 post 前或添加 rollback。`behavior_tests.rs:335` 明确固定了这一旧行为。`mod.rs` 既有“任何拒绝字节不变”的总述不能解释为覆盖所有 serde 异常 state；它与该旧边界的矛盾不是本批新增缺陷，最终汇报必须保留限制。
- **恢复后 ECL 接受集：** `ecl.rs:97` 的 initial_allowance_target 直接借用 stage1_default，刻意不调用 validate；空列表依旧算出零，issue 依旧仅产生 disbursement event。`behavior_tests.rs:229` 覆盖此路径。EclPolicy 的 validate 仍先 stage1 后 lifetime；scenario 内仍 weight→PD→LGD，列表先逐项校验后权重和。checked factor→gross contribution→scaled add→单次 rhe_div 的算式、错误 op/detail 与顺序不变。
- **private 字段与 serde map/sequence：** BankLoanState 的 derive、11 个字段名称/类型/排列及 serde attribute 与基线一致，仅 rate_bp/counterparty/allowance 的 Rust 可见性改变；serde derive 的同模块实现仍能访问这些字段。DepositState、EclPolicy、BankBooks 的字段和 derive 亦未改动。因此未发现 map 或 sequence 接受集合变化；不得把 private 化描述成恢复时新增业务校验。当前新增 tests 使用 JSON map，没有专门的 sequence 输入 fixture；序列接受范围在本审查中仅由未变的 derive/字段顺序静态核对，不是动态验证结果。

## 验证局限与后续证据

未运行新增及既有测试；未独立验证 TDD 红阶段记录。既有 failure tests 主要使用正常业务构造的 state；本批只新增 recovery 的 serde 极值部分 apply 保护，没有对 deposit/loan accrual 的 post 成功后 checked add 溢出、跨多个合同的部分 apply、以及 sequence serde 接受/拒绝逐项增加动态 fixture。上述路径源码顺序保持原样，未据测试缺项推断回归；若 worker 汇报它们已动态验证，必须提供具体测试证据。实现者后续修改任何已审文件时，应按增量 diff 再复核。

本次审查快照 SHA-256：

| 文件（`packages/engine/src/company/bank/`） | SHA-256 |
|---|---|
| config.rs | 09f16f4d5675f33325a228e586d428f3ec4958e74db9bd8176c0cb7ede450aad |
| deposits.rs | 815f7b3331bce467772fe9a3b40d796ad850cb2dfe3cbce3887e2977eaea39bd |
| ecl.rs | 24d4a70aed59e0e5b6f7778f0fe15036d9674048459437358cb843e1deede394 |
| interest.rs | 43addf8f097424a186a71cef8dfa2fe14a29fa9d8ca34f8e894ef0317dfead3d |
| lending.rs | eb0a6bc0f180499f8c72133de0b459250be811488c539d6cd48e85b1758b95c4 |
| loans.rs | 33f3337bad79e3e4c0c5fcebece5b7da02a038b15edc123f883bd87a0d13ec1b |
| mod.rs | ffa6307a60577c308e77361dc01d6afffcd68af3a04266e259115a93253bd50d |
| writeoff.rs | a0ea1de9188f62f43010732191fd6c82b812ef2808a51744ef0fa3164a4a0d81 |
| behavior_tests.rs | cb1ef9f23b21b4300069c4f1ab036adf41d0b68651d9a36ebeff6d5502365a19 |
