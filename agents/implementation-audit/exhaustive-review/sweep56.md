# sweep56：工商/保险 OOP 实施与复核记录

## 基线与全文范围

生产源码基线 `b76ece3`（审计 merge 的产品源码相同）。已读 AGENTS.md/principles，并逐篇连续读至 EOF；未修改产品、执行 Git 写操作或运行测试。

| 文件 | 行数 | EOF |
|---|---:|---:|
| `agents/oop-refactor-implementation/domain/industrial-review.md` | 67 | 67 |
| `agents/oop-refactor-implementation/domain/insurance-result.md` | 47 | 47 |
| `agents/oop-refactor-implementation/domain/insurance-review.md` | 62 | 62 |

共 176 行。工作记录主要承诺等价 owner 迁移，不能自行取消正式公司计划 K2/K3/K4 的交易/经营/报告约束；历史 hash/短测试结果仅绑定记录所述版本，不当作本轮实测。

## 逐章条款矩阵

| 文件/原文行号 | 条款状态及当前源码/caller |
|---|---|
| industrial-review 开头 1–9；版本绑定 11 | 基线与完整diff/未跟踪测试覆盖是历史复核范围。当前 `company/industrial/mod.rs:69` IncomeTaxPosition、`:70` LoanPortfolio 真正装配，不仅存在无caller对象。原hash不在本轮重新计算，不宣称重新冻结。 |
| industrial-review 结论 29；大A/会计 33 | ACT/365F、费用、资本科目、游戏税务政策与金额单位未因owner迁移变成证券制度；`industrial/loans.rs:86` 唯一贷款组合、`industrial/expenses.rs:260` 税务预览调用原 policy.compute。TaxPolicy/Books/ContractBook仍各有owner，未另建现金账。 |
| industrial-review 必要性 39 | OpeningReconciliation 实际由 `industrial/mod.rs:89` 构造并依阶段处理；LoanPortfolio方法供原borrow/accrue/pay/repay API消费；IncomeTaxPosition `expenses.rs:246` 唯一持有loss_pool，`:279`提交ending_pool。不是未完成的规划类骨架。 |
| industrial-review 边界 43–49 | lazy合同预览、post后子账提交、零分录/事件槽以及serde transparent保留原行为。年度税务 `expenses.rs:284` 仍汇总全年收入费用，未增加同年幂等；原记录明确保留旧行为，不把本次等价迁移报告成修复。正式G35的税务日结调度缺口仍成立。 |
| industrial-review 有效发现 52；IR-01 54–61 | 记录caller误归BankBooks已经关闭：`industrial-result.md:23`现在仅写dispatch Industrial分支，`:52`有更正复核；实际 `operations/dispatch.rs:41`–`:42` 调IndustrialBooks::accrue_interest，`:44`–`:46`是银行自身两类利息。无需修改BankBooks来配合旧错误文档。 |
| industrial-review 局限 63–67 | positional serde/多合同首错测试为未来改写时的保护建议，原文没有确认当前实现变化，不列新产品缺口。没有用静态review冒充新增case本轮运行通过。 |
| insurance-result 开头 1–6；逐动作 8–17 | config开局守卫、measurement/claims唯一owner已装配；`insurance/groups.rs:249`、`:311`为纯preview，`:389`、`:414`为apply；release_service/remeasure caller `service_release.rs:97`–`:99`、`remeasure.rs:100`–`:102`。`premium.rs:62`原构造API继续建组，不要求仅为“有diff”修改premium。 |
| insurance-result 领域与失败 19–24 | CAS25及D1–D5游戏假设沿用当前 `docs/company-accounting.md:91`–`:111`，不新增官方核验主张；保费负债/赔案与付款分开 `premium.rs:62`、`:174`和`claims.rs:111`、`:164`。post失败不写子账；极值apply失败部分写入为已明确继承边界，不把OOP等价重构算新故障。 |
| insurance-result 短行为测试 26–43 | 10 case与正常/拒绝/serde/极值/失活carry覆盖已有；当前生产入口实际消费新owner。测试first与worker未跑baseline是历史实施限制，后续集中短测结果记录不能升级为完整回归。 |
| insurance-result 最终门禁 45–47 | 记录明确只有指定短case与独立静态审查完成、无完整回归。本轮未重新执行，不新增“所有验收已完成”的主张。 |
| insurance-review 开头 1–8；版本绑定 10–24 | 7文件完整hash绑定/新增behavior_tests全文是历史scope，不以旧hash代替当前源码。当前Snapshot在 `groups.rs:428`，保留扁平serde接口。 |
| insurance-review 结论 26；语义依据 30 | owner迁移不更改证券交易制度、币股单位或资金来源；公开正式D1–D5边界继续保留。TermProtection与明确期限仍是支持范围，不因重构说明允许无限期生成赔案（下一节候选）。 |
| insurance-review 范围/复杂度 38–44 | measurement唯一持有计量/进度；ClaimRegister唯一赔案映射，`claims.rs:153`与`:207`通过register写入；旧InsuranceBooks API/operations caller `operations/insurance.rs:49`、`:59`、`:71`、`:75`、`:76`真实消费。没有无用第二账本。 |
| insurance-review 跨层/错误 46–54 | Snapshot derive相同、24字段、既有carry单位；preview正CSM/正loss才release `groups.rs:266`、`:284`；零组件保留失活carry `:275`、`:293`。apply顺序 `:395`–`:409`、`:415`–`:420`先余额再累计再carry/units与原记录一致。各结果是静态证据，不是本轮运行证明。 |
| insurance-review 剩余限制 56–62 | 原始JSON输出顺序/sequence长度/后续累计溢出组合仅覆盖建议，未确认语义回归。复核未编译与实施记录后续集中编译结果时点不同，不按表面措辞算矛盾。 |

## 新候选 S56-C1：保险经营 caller 在保障结束后仍无限生成新赔案

**候选状态：源码静态可达，供主审独立裁定；未运行场景，不宣称默认工商Session已经触发。**

- 正式契约：`docs/superpowers/plans/2026-09-10-company-information-npc-intentions.md:105` 保险支持明确期限非分红合同；`docs/company-accounting.md:102`同样明确。本批 `insurance-result.md:17`声称真实日常经营caller仍通过InsuranceBooks执行每日释放与赔案，`insurance-review.md:44`确认caller接线。正式计划优先于“此次没有改经营代码”的重构范围说明。
- 实际 `company/operations/insurance.rs:63` 收集全部历史组，每日逐组处理。`:68`读units_remaining，`:70`仅用remaining>0限制服务释放；随后`:73`赔案门槛独立为 elapsed>0 && claim_every_days>=1 && elapsed%claim_every_days==0，没有coverage_end或保障未结束判断。`:74`按当前elapsed生成新的CLM ID，`:75`记录当前日发生，`:76`尝试付款。合同组没有在遍历前因保障结束删除或过滤。
- 最小静态场景：同一明确期限10天的TermProtection、claim_every_days=5；第5/10天可以产生该版本保障日赔案，但第15/20/25天仍继续生成新的赔案与费用/现金请求，即使责任单元已全部释放。`premium.rs:92`要求end>start、`:103`以end−start确定units_total，期限事实本来可读。`claims.rs:118`–`:138`只校验金额、组存在与赔案重复，不会替caller阻止无限新造。
- 这是日常经营caller的合同期限消费遗漏，独立于G36“无法自定义保险公司进入完整Session装配”；当前底层四行业operations是存在并可调用的路径。不声称所有保险会计handler缺失。
- 正式D1–D5 GMM简化（`company-accounting.md:107`）与保障日确定性赔案模型未授权到期之后无限发生新赔案；后续用户决定若有不同明确契约，应按决定重新裁定。修复方向宜在经营模型区分新事故发生期限与旧赔案结算，不能把public record_claim报送日期或到期后支付合法既有赔案一概禁止。
- 建议代表性短测以一个组、少量自然日越过coverage_end，核第一个到期后赔案槽不再创建新Claim、旧未付Claim仍可支付，并固定结束日包含/排除的现行游戏定义。此分片未新增或执行测试。

## 其余候选的反证与归属

1. **IR-01 caller记录已修。** 银行贷款资产与工商借款负债的错归属不再保留为未完成项。
2. **G35/G36保持。** 已有LoanPortfolio/IncomeTaxPosition/ContractMeasurementState方法不等于日终所有调度、四行业自定义Session与公共报表闭环都完成；owner重构没有补这些历史缺口。
3. **底层极值apply部分写入不新列G。** 本工作记录明确继承、总账第5节已有同类边界裁定；正式K2:86要求Journal批次原子，不自动扩张为每个底层handler所有可编辑极值均强原子。没有新增正常默认局可达证据。
4. **失活carry不是新的可实现任务。** CSM重估归零后旧carry可保留已被新测试诚实固定；余额与未来责任已归零时没有当前caller把失活carry当现金或保费。可澄清概括归零文案，不凭该记录强行改会计模型。
5. **同年重复CIT非幂等暂不单列。** 当前正式生产调度原本缺失在G35；重复手动API调用旧语义在本review明确保留。补调度应先确定税前口径与年度一次执行边界，不能以重构记录授权新增税务路线或默改税率。

本分片只形成新候选S56-C1；所有历史短测/静态复核结论保留原范围，不冒充本轮实测或完整领域验收。
