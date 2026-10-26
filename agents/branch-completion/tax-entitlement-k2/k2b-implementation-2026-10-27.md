# TAX-K2b 条件收缴与历史重放实施

日期：2026-10-27。唯一writer `/root/sol61_tax_k1_resume_fresh`，独占`.worktree/tax-entitlement-k1`。
基线为已独审K2a `a51e0939c0a3fac0cd5e754c22e4fe898fdd145d`；v2合同`0dd70f66`/SHAf7aa21a3已批，
K2a终裁见根`tax-entitlement-k2a-review-2026-10-27.md` SHAe1f1c383。

## 本片实现

- 同一有序侧事实流新增typed Collect，明确available_cash、cash_collected、bonus_collected；
  两个实收分额的和派生总额，未写法定Cash/Bonus扣收优先级。
- 每个Collect必须各分额非负/不超过当时kind欠税，合计=min(联合欠税,给定available_cash)。
  没有分额的输入严格拒绝，不凭合计余额遮住某kind超收。错误带event、两kind当时欠税/实收和可用金额。
- 每个book操作后再按同prefix侧事件顺序执行，前缀限制排除后续同日付款；
  Realize→Collect与Collect→Realize按输入顺序分别评税，不能借未来所得。
- 原样typed重复保首次位置后忽略，异事实冲突；同一个真实book collection事件不能在侧流二次入账。
  Collect身份域不混收入事件域，不以字符串前缀猜typed动作。
- legacy Cash的每笔真实收缴先减所有先前Cash实收（既有legacy和侧流Cash分额），
  拒绝Cash先条件收尽→Bonus激活→legacy Cash再收的跨kind遮盖。允许真实旧Cash少收且不改历史快照。
- 已收Cash/Bonus仅为重放期间派生整数计数，不新增可编辑累计税额权威，不写生产税账字段。
- `MappedTaxAssessment::prepare_collection`纯函数输出候选collected/remaining_cash/outstanding/needs_funds；
  单独prepare无账户副作用、不追加事实、不减少下一次评估税额。只受理非负Money与整分assessment。
  税债可保持i128 exact整分；可用/实收仍受Money i64边界，checked不截断大欠税。

本片actual collection仍是**显式条件输入**，available_cash不是已查真实账户的承诺。
真实D1a inspect before/after与WithholdingTax/ShareSplit来源、commit成功P、受控分kind归属规则、
真实税务日/physicalcapture原R和SaveSlot/Web恢复仍待生产接缝。本片不撤R2 Bonus门、
不改旧生产outstanding/collect_due或账户，不用helper存在宣称生产个人Bonus或税bug已完成。

## TDD与证据

红测试/stub提交`232d0183`：用collection过滤实际运行11case（含7个原相关case）中4红7绿。
4红分别为prepare应收300实0、实际条件收300后应欠1700实2000、同prefix收缴借未来激活未拒、
Cash条件先收后legacy双收未拒。失败落新行为断言，fixture准备均成功，exit101。

首轮最小实现全99/99绿；补边界后最终全105/105绿（新增K2b10项＋已审K2a/K1/旧95项），
真实binary报告1.08s，8threads，整命令/每case均被10000ms外部进程树deadline约束。
最终binary SHA `a89aaa61e56c9b59e49942409b88706df15409d4cd94edd881d889571de73d55`，
26628680B；构建2.875s，codegen32，300000ms外监督，50ms进程树采样峰RSS497291264B/23threads。

新增10项代表case覆盖部分/继续收缴、prepare无副作用、同prefix前后激活顺序、
分kind归属不猜优先级、条件Cash与legacy交错双收、负/超额/少收/冲突/重复真实来源拒绝、
后续同日Cash付款不能遮盖早收、部分已收后book+侧事实恢复继续、缺归属/篡改额拒绝、
超过i64的exact欠税与Money最大可收，非法prepare负可用/非整分拒绝。

每轮13个真实源＋5个原rlib依赖的before/artifact/run JSON、实际argv/exit/wall和原始build/run日志
逐字封存在`k2b-evidence/`，manifest密封大小与SHA。源码/依赖/binary运行前后校验不变。
每标签harness源码/binary保在根`.tmp/tax-entitlement-k1`，旧红产物不覆盖；没有Cargo/冷target、
完整engine回归、Web/生产旅程或lint/Clippy通过声明。

## 冻结独审门

本片源增量仅entitlement_units.rs、conditional_bonus.rs和conditional_bonus_tests.rs，
未改旧cash_dividend_tax.rs共同evaluator、其他domain、REG物理或main树。
当前仅实现和有限105测试通过，待原未实施Sol完整diff独审；通过后再给main唯一writer准入，
严格生产来源/税务日期/capture/Admin真实安装/SaveSlot与Web闭环不由本片代签。
