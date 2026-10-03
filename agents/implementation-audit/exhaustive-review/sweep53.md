# Sweep53：bank 重构及 Domain 协调工作记录全文复核

## 基线与全文覆盖

- 目标源码 `b76ece3`；已对照当前后续文档HEAD，bank/operations相关生产源码无差异。
- 连续从首行读至 EOF：`agents/oop-refactor-implementation/domain/bank-result.md` **66 行**、`bank-review.md` **39 行**、`coordination.md` **34 行**，共 **139 行**。
- 已沿用根AGENTS/principles/architecture/open-questions要求；正式产品范围对照公司计划K2/K3及任务9/26/27、`docs/company-accounting.md`。这些工作记录是行为保持重构记录，不能覆盖正式产品需求或把原有缺陷改写成正确实现。
- 引用的 `agents/oop-refactor-audit/challenge-2026-10-03/action-index.md` 在本审计worktree不存在；同样未发现该组completion-ledger及validation JSON。记录中成功状态只作为历史汇报，不冒充本轮读到/运行的日志。链接缺失属于证据归档边界，不新开产品代码功能。
- 本轮只新增此审计文档；未改产品、未运行Cargo/测试/浏览器/长验收、未写Git。

## 逐章条款—当前 caller 台账

| 原文章节/行号 | 当前生产 owner→caller→消费 | 判定 |
|---|---|---|
| bank-result 引言 1–9：bank范围、政策依据、测试/独立审查与最终验证 | 当前bank目录仍为独立子账，正式公司计划任务9 `:342`要求BankBooks经营原语；reviewer身份与静态边界由bank-review记录 | 源码与历史审查记录存在；没有本轮red/green/编译证据。root历史运行结果不等于现在重新验证 |
| domain-N04 11–18：BankConfig自有禁止科目守卫、ECL先错 | `bank/config.rs:30` check_opening_lines扫描既有10科目→`bank/mod.rs:110`先ecl.validate、`:111`开局守卫、`:113`post_batch | 已真实接BankBooks::new，非只有receiver定义；禁止清单没变允许清单，首个非法行仍顺序扫描 |
| domain-R2-N10 20–28：DepositState提取守卫/计提预览与BankBooks保留过账 | `deposits.rs:93` preview=min(through,maturity)、`:126` validate_withdraw→`:259`withdraw_deposit调用；`interest.rs:136`预览→`:162`统一post→`:165`apply_accrual | receiver已被实际生产handler消费；款项现金约束仍post检查，余数、日期及资金流保持。极值post后部分apply属于下文C01，不据“保持”核销原子性 |
| domain-R2-N11 30–39：EclPolicy/单次slice借用、权重/算术/舍入 | `ecl.rs:89`两表validate；`:97` initial_allowance_target→`lending.rs:36` issue_loan；`:188` assess受检借用→`:189`未知贷款→`:192`阶段守卫→`:193`目标；`:137`checked i128累计、`:166`单次rhe_div | owner/caller已闭环。无复制两政策表或新权威金额；恢复后initial故意不重验，保留接受集不等于政策校验已覆盖恢复，见C02 |
| domain-R2-N12 41–51：BankLoanState private字段、constructor与七handler | `loans.rs:28`字段private、`:53`new接受day-one；`lending.rs:65`真实constructor、`:87`principal guard；`interest.rs:48`preview、`:90`利息守卫；`writeoff.rs:25`核销preview、`:75`回收守卫；ecl`:192`assessment | 具体receiver都有真实BankBooks caller；LoanWriteOffSnapshot只分录事实。上层operations/dispatch确实调用相应BankBooks API，但默认会话四行业能力仍G36，不以子账重构宣称会话已支持Bank |
| 同章 48–50：validate→post→apply、Stage1/2本金/Stage3净额、核销/回收 | `loans.rs:132`分阶段基数、`:270`净额非负；`writeoff.rs:48`post→`:59`apply_write_off；`:77`post→`:92`apply_recovery→`:94`flow；`loans.rs:339`先减recoverable后加allowance | 保持原次序；核销后零分日期/事件推进仍经interest统一post/apply。原有极值非原子结果明示保留，不把零分或顺序测试当全部恢复状态正确 |
| 短验证入口 53–58：9个case、engine --lib、多核10秒、原suite | `mod.rs:33`cfg(test) behavior_tests，`behavior_tests.rs:73`至`:335`9个test；既有bank_accounting suite仍在 | 测试真实定义存在；本轮未跑。静态测试源码不能证明红阶段、通过数、具体平台或sequence接受集已动态覆盖 |
| 语义与范围核对 60–66：银行/证券账户分离、分/bp/余数、不新增高级模型、serde保留 | BankBooks持有Books/deposit/loan/counterparty/ECL，非证券account；`loans.rs:361`ACT/365F分母；无股票跌幅算PD或隐式补钱入口 | 当前主要单位/边界一致；原有提前取款/不折现等已登记简化不是本批新功能。private不改变derive serde，不能称新增恢复校验 |
| bank-review 范围与结论 5–9 | 9源码文件/原caller现在存在；实际receiver如上 | 独立静态结论为“无本批新增阻断finding”，不等于没有原有缺口或所有当前接受状态安全 |
| 三门核对 11–15：大A、必要性、顺序与复杂度 | 上述账套/单合同/ECL receiver归属；元/股交易层无新增修改；正式金融准则依据沿用仓库登记 | 局部行为保持与最小范围可静态对应；本轮未联网复核CAS全文，没有将会计简化称完整银行实务 |
| 特别复核 17–19：post/apply异常部分提交 | `interest.rs:70`后多loan apply、`:162`后多deposit apply；`loans.rs:283` / `deposits.rs:163`checked add；`writeoff.rs:77` / `loans.rs:339`回收先提交再局部失败 | 原记录明确残余仍在，C01；不是本批新增。不能因保护测试锁定旧行为就把“任何拒绝字节不变”总述当成立 |
| 特别复核 20：恢复后的ECL接受集合 | `ecl.rs:101`直接stage1借用，BankBooks derive Deserialize (`mod.rs:96`)；`behavior_tests.rs:229`从JSON把stage1设空后issue仍成功且allowance0 | 原记录明确保留的异常政策接受边界仍在，C02；政策构造校验已有，恢复路径校验不能由构造入口核销 |
| 特别复核 21：private字段与serde map/sequence | BankLoanState11字段及derive保持；新增测试JSON map，未见sequence fixture | 仅静态接受形状证据；sequence验证是明确证据债，非已确认生产回归/必增功能 |
| 验证局限与后续证据 23–25 | 记录要求deposit/loan accrual极值、跨合同部分apply、sequence输入需具体证据；当前behavior_tests未补这三类完整动态fixture | 仍是未验证边界。原文并非授权改变旧接受集或每项必须新增模型；若报告这些已验证需要日志，不补造结果 |
| 审查快照 27–39 | 文件对应当前bank路径；sha清单是历史冻结证据 | 本轮未重新哈希核对全部快照，也未认定历史SHA失配；不因存在表格宣称产品行为通过 |
| coordination引言 1–8及worker表 10–20：39动作闭环/104case/集中验证 | 实际bank动作可对应，其他worker只由该表汇总；各领域实施记录存在；正式计划功能缺口仍台账G系列 | 汇总不是新增产品需求，不替其他领域逐行复核，也不以39/104数字核销G36等。历史branch名不构成新建/提交Git授权 |
| 跨组接线 22–24：N07机构观察与reconciliation receiver | `session/institutional_behavior.rs:23`调用observe_institution_position_dated；`experience/feedback/lifecycle.rs:43`双map transition；`session.rs:1547`reconcile→`:1582`clear_stale_institutional_holding | 真实生产调用已接；不能因旧dated API有实现就冒充新transition未接。散户G08 legacy链仍另记，不用机构receiver核销 |
| 跨组接线 25：N41 CausalCollector receiver、连续/竞价不同失败面 | `session/execution/records.rs:17`record_continuous_fill；`pipeline/auction_day_end.rs:1937`record_auction_fill；collector `diagnostics/causal.rs:141` / `:168`各持有facts/index更新次序 | 真receiver接线存在。连续checked add失败保留已append事实、竞价校验失败不append属于原动作保留的差异，不强行统一语义；G37根诊断订单关联不由此核销 |
| 跨组接线 26–27：A03账户getters/restore、Position重建、应用save先验事实 | `account.rs:141`restore_balances返回unit、`:622`Position::from_restored_parts返回Self；`session.rs:2720`先validate_save_slot→`:2725` / `:2735`应用balances→`:2743`Position重建 | 当前生产restore已消费；不能要求constructor再加校验导致首错漂移；fixture-only API无需虚假生产caller |
| 跨组接线 28：N06税policy receiver与工商caller | `industrial/sales.rs:76`vat.output_vat_on；`purchasing.rs:76`split_input_vat；`expenses.rs:131`IncomeTaxPosition预览借policy→实际分录/apply | domain归属已有；默认日终所得税调度G35未因库级accrue_income_tax receiver改变而完成 |
| 验证说明 30–34：行为保持测试baseline通过、编译红≠行为红、冻结后集中复验 | behavior_tests明确保护旧API边界；历史集中编译/短测见final-summary记录 | 原记录诚实承认不提供行为红。不能反推伪造TDD，也不能把这类测试的已存在断言改弱来掩盖现行缺口 |

## 正式计划与生产层级复核

- 公司计划 `2026-09-10-company-information-npc-intentions.md:100`与任务9 `:343`要求银行合同原语；这些原语都存在。`operations/bank.rs:58`吸收deposit、`:73`放贷、`:104`fee、`:117`信用重估；`operations/dispatch.rs:44`贷款/存款计提、`:96`贷款收息、`:105`收本都消费BankBooks。故候选“重构只把方法写出来，上层完全不用”有真实caller反证，撤销。
- 部分原语如withdraw/write_off/recover不是自动每日日程必然触发；正式计划要求支持事件原语，不等于每个每日advance必须自动执行核销。不能为凑生产caller编造违约、核销或用户提款。默认SessionSetup仍固定Industrial及非工商封账接口的问题已G36登记，不重复新编号。
- 银行固定日日程是 `docs/company-accounting.md:130`明确游戏假设。任务9 `:346`也明确默认股票不增加银行股；现行G36要求自定义四行业会话闭环，不要求默认增添银行股。
- 本轮“重构完成”仅说明owner/caller动作闭环，不能取消正式任务27 `:509`完整公司/政策存档校验和原子恢复要求；下面残余按此界限登记。

## 新候选与已知残余，暂不冒充默认会话复现

### C01：BankBooks总述的原子拒绝承诺与极值恢复后处理器部分提交矛盾

- 工作记录明确原文：bank-review `:19`、`:25`，bank-result `:50`承认回收overflow后总账/event_id已提交、recoverable已减少、allowance溢出、flow未登记；正式库模块总述 `bank/mod.rs:8`至`:12`却仍称任何拒绝账套/子账字节不变。
- 当前生产事实：`writeoff.rs:77` post_with_commit成功后，`:92`apply_recovery调用 `loans.rs:339`先减recoverable，`:340`checked加allowance可能失败，`:94`flow未到；`behavior_tests.rs:335`实际构造serde极值并锁定partial状态。计息处理器也在整体post后逐合同checked apply，源码仍保留类似后失败窗口。
- 与现行批次关系：这是重构特意保留的原行为，不能报为重构新回归；也不能报告“所有拒绝原子”已实现。需要主控将库级异常恢复接受集合、调用外层是否有事务影子、真正公共存档到BankBooks的可达路径与正式原子性要求一起判定。
- 建议状态：已有残余/契约冲突候选；不在本组直接授权改变已冻结行为。当前默认会话只装配Industrial，尚未运行编辑Bank存档到UI链的复现。

### C02：serde恢复BankBooks后可使用未重新校验的初始ECL政策

- 原文bank-result `:37` / bank-review `:20`刻意保留issue_loan不重新validate的接受集合；正式计划任务9 `:348`非法PD/LGD拒绝，任务27要求完整恢复校验。
- 当前 `BankBooks` derive Deserialize (`mod.rs:96`)不经new；`EclPolicy::initial_allowance_target` (`ecl.rs:101`)直接借政策切片；`lending.rs:36`消费该目标。现有 `behavior_tests.rs:229`恢复空stage1情景后，policy.validate本会失败，issue仍成功零准备。
- 反证与限制：普通BankBooks::new和assess_credit都真实validate，因此不是完全没有ECL校验。重构没有授权改变原serde接受集合；不要求每次issue重复启动时已经验证过的整政策。需确定公共恢复入口是否完整重验Bank政策，不能从private字段/shape保持推出业务事实有效。
- 本审计看到 `session/persistence.rs:977`公司域检查覆盖映射/时钟/scheduler/public库，没有在该函数内重验Bank政策；但尚未穷尽上层公司恢复与所有跨层守卫，因此保持候选，不能称已运行公共坏档接受复现。

## 证据债及核销

- bank-review未承诺已经跑sequence、跨合同计提后失败、极值accrual动态fixture；当前九case只明确覆盖其中回收极值。它们是测试证据缺项，不能自动逐项新建产品功能。
- 历史action-index/completion-ledger/validation JSON缺席需归档核对；不把历史工作记录的断链当产品源码遗漏，也不据历史“通过”替本轮未运行的结果背书。
- Bank合约的分/bp/余数、存款负债/贷款资产、客户流动性与证券投资者资金边界在上述实际caller中保持；本轮未新增制度判断或重新联网核验官方准则。

本批139行全文覆盖后，未确认新增默认会话生产缺口；登记2项原记录已明示但正式契约仍需收口的库级候选，核销receiver-only候选，保留测试/证据归档边界；G35/G36/G37/G08等既有缺口不由行为保持重构状态核销。
