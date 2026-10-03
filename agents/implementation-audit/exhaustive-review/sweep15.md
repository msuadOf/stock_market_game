# Sweep 15：历史 account / GameConfig 全文复核

## 基线与阅读记录

- 任务基线为 `b76ece3`；读取时 worktree HEAD 为 `4ad5a2e`。只读 `git diff --name-only b76ece3 HEAD -- packages apps docs/decisions docs/trading-rules.md` 无输出，所查产品与规则文件未变。
- 已读根 `AGENTS.md`、`docs/principles.md`、`docs/architecture.md`、`docs/open-questions.md`；领域解释参照 ADR-0005 当前 T+1 修订与 `docs/trading-rules.md`，未联网重新确认官方材料。
- 三篇文档均按顺序连续读到 EOF：`docs/superpowers/plans/2026-06-29-account.md` **860 行**（分段 1–290、291–580、581–860）；`docs/superpowers/specs/2026-06-29-account-design.md` **152 行**；`docs/superpowers/specs/2026-06-29-gameconfig-design.md` **120 行**。
- 对照总账 R07/R12、Q01/Q06，以及 `coverage/r07.md`、`coverage/r12.md`、`reaudit-core-contracts.md`、`reaudit-foundations.md`。测试仅阅读源码；未运行产品、cargo、构建、浏览器或长测试。仅运行一次纯 Node 算术表达式核对新候选示例，见下。
- 本文件只新增审计证据，不修改产品、测试、正式规则或 Git 状态。实现状态仅限具体条款，不代表模块完整验收。

## Account plan 全任务覆盖

| 原文位置与承诺 | 当前实现与真实消费 | 状态 |
|---|---|---|
| L1–33 前言、Global Constraints、文件布局：统一账户，整数分、显式拒绝、费用不进成本 | `packages/engine/src/account.rs:95` 的 Account 及 `:103` 的 COW state；现金 Money、成本 i64；`:326` / `:435` 先算后写；费用真实来源为 `session/pipeline/transition.rs:150` / `:171` | 核心已实现；单文件/公开字段组织已演进。TDD、提交的历史过程不能靠当前源码确认。 |
| Task 1 L37–145：Strategy、Intent、MarketView、Rng 骨架 | `strategy/mod.rs:98`、`:135`、`:169`、`:276`；`strategy/state.rs:33` ProductionStrategy；`account.rs:207` 安装；`session/pipeline/npc_decisions.rs` 消费真实策略决策 | 已被完整策略体系承接，不因旧 strategy.rs 消失误报。 |
| Task 2 L147–246：错误、账户分类、代码类型、导出 | `account.rs:32` StockCode、`:36` AccountKind、`:49` AccountError；`lib.rs:38` 导出。资金、持仓、费用、非法成交、预留错误均保留显式类型 | 已实现；增加错误类型不属于漏做。 |
| Task 3 L248–380：Position、净投入/持仓、负成本、半偶、可卖量 | `account.rs:609` 的私有事实；`:650` checked 可卖；`:658` 成本、`:673` 纯整数 half-to-even；`tests/account.rs:80` / `:102` / `:110` / `:117` | Rust 已实现；Web 消费端存在 C15-01，不能把 Rust 单测核销跨层显示。 |
| Task 4 L382–475：构造、无策略、策略注入、只读访问 | `account.rs:194` new、`:207` set_strategy、`:222` has_strategy、`:242` sellable_qty、`:251` cost_price、`:124` cash / `:132` positions | 已实现；玩家策略默认 None，COW 私有状态替代 public 字段。 |
| Task 5 L477–597：买入校验、佣金、成本累加、T+1 | 库入口 `account.rs:287` 先 config.validate/正价正股；生产 `settlement.rs:54` 从 Fill receipt 取量/额、`:119` apply_settlement；`account.rs:326` 买入含过户费，checked 数量/成本后 `:371` 才写 | 已实现；入簿前另在 `account_validation.rs:705` / `:740` 按真实资源拒绝不足资金。 |
| Task 6 L598–703：超卖拒绝、佣金/印花税、回收额、清仓删除 | 入簿前 `account_validation.rs:722` 持仓预算拒绝；结算 `settlement.rs:127` → `account.rs:435`；`:448` 净额，`:466` checked 回收额，`:479` 清仓删除 | 已实现；无仓返回 InsufficientShares 与 plan L606 一致，不要求恢复 NoPosition 分支。 |
| Task 7 L706–833：统一入口、市值/盈亏、导出、clippy/build/regression/提交 | `account.rs:519` apply_trade、`:536` batch；`:575` market_value、`:588` unrealized_pnl；`lib.rs:24` / `:30` / `:38` 导出；真实结算走 receipt 而非旧同名入口 | 核心已实现；Result 显式传播溢出优于旧 ZERO fallback；Web 派生消费 C15-01。验收命令本轮未执行。 |
| Self-Review L835–860：覆盖、类型、total_assets 延期 | `session.rs:1471` account_equity 实际以 cash+全部持仓最新价市值聚合；`:1521` 初始化经历、`:1619` 更新观察消费；`observation.rs:364` 风险聚合；plan L847/L860 明确顶层承接 | 同名 total_assets 缺失已反证；保留明确延期解释，不另报。 |

## Account spec 全章覆盖

| 原文位置与承诺 | 当前证据 / 分类 |
|---|---|
| §1 L10–14 背景、统一账户、账务独立 | `account.rs:95` 与 `session/pipeline/settlement.rs:96`：账户只账务，策略/撮合在上游；已实现。 |
| §2 L16–27 六项核心决策 | 现金/可卖拒绝及成本见上述 Tasks 3/5/6；费用独立于 gross；T+1 日界见 `auction_day_end.rs:2157` → `account_book.rs:139` → `account.rs:227`。旧 T+0 产品旋钮由 ADR-0005 L73–75 明确取代。负成本显示 L25 见 C15-02；半偶跨层见 C15-01。 |
| §3 L29–67 类型/API、Trade方向、资产聚合 | 身份与分类保留、Position 分单位事实保留；receipt 自带账户/方向，`settlement.rs:82` 按 side 汇总再结算；`account_equity` 等价资产聚合已接。cost/pnl Rust满足，Web见 C15-01。 |
| §4 L69–87 买卖步骤、费用不进成本 | `account.rs:332` 总现金支出含费用，`:365` invested只加 gross；`:448` 卖出净现金，`:466` recovered只加 gross。生产费用按委托累计额在 `transition.rs:150` / `:171` 算，汇总在 `settlement.rs:151`；已实现。 |
| §5 L89–97 错误处理 | `account.rs:49`、`:295` / `:405` / `:501` 边界错误；`settlement.rs:120` / `:128` 映射 invariant；`account_settlement.rs:93` 准备成功后 `:118` 提交，未见静默吞错。 |
| §6 L99–105 明确模块边界 | 撮合/行情/策略职责分开；显示负成本约定不属于 Account，但仍须核对前端，见 C15-02。 |
| §7 L107–121 十二项测试矩阵 | 构造/None `tests/account.rs:126`；买入 `:147`；资金不足 `:213`；加权 `:195`；卖出/清仓 `:279`；超卖 `:328`；T+1 `:265`；负成本 `:102`；派生 `:443`；溢出 `:245` / `:493`；Position serde事实通过 snapshot/save 映射保留。无执行结果，未把测试存在写成通过。 |
| §8 L123–133 文件/策略前置 | 已演进为模块目录与 StoredStrategy；`strategy/state.rs:33` 与 `account.rs:207` 实際连接，非占位。 |
| §9 L135–142 验收 | 源码整数/费用/拒绝/T+1主干满足；本轮未跑 test/clippy/build，无法新增验收通过结论。 |
| §10 L144–152 trait范围/纯整数舍入 | Rust trait已扩展、舍入 `account.rs:673` 支持负数。Web派生不共用该语义见 C15-01。 |

## GameConfig spec 全章覆盖

| 原文位置与承诺 | 当前实现 / 调用 / 状态 |
|---|---|
| §1 L10–20 比率/配置动机、旧 Q8/Q9 阻塞 | 目前策略/市场已存在；Q8/Q9在 open-questions 已解决，不能把旧阻塞重新登记。 |
| §2 L22–28 纯配置、f64仅比率、Money金额、默认集中、构造校验 | `config.rs:79` serde结构；`:106` new调用`:132` validate；默认集中`:189`。金额为 Money、比率 f64；已实现。跨端金额范围仍 Q01。 |
| §3 L30–70 字段/错误/new/default/费用 | `config.rs:49` 错误类型、`:79` 字段、`:106` new、`:189` default、`:212` commission取max、`:229` stamp_tax无floor；`:234` transfer_fee 后续增加；`session.rs:996` 再校验，`:1002` / `:1008` 约束正式A股参数；佣金真实经 transition/receipt进入账户。 |
| §4 L71–75 范围排除 | UI设置非本config模块实现范围；撮合/持仓/行情另有生产实现；不据UI缺按钮认定配置schema未实现。 |
| §5 L77–82 显式错误/受控默认/透传 | validate Result、默认 expect有上下文、commission/stamp_tax透传MoneyError；当前账户库入口每次校验config，session外部setup也重校验；已实现。 |
| §6 L84–101 T1–T6/边界矩阵 | `tests/config.rs:10` 错误显示、`:103` serde、`:143`裸整数、`:174`起new边界、`:528`默认、`:556`稳定、`:576`佣金floor、`:592`费率、`:630` / `:645`印花税；只源码证据。原文L99把10000元×0.00025=2.5元称“大额”且期待250分忽略5元floor，是旧算术/例题错误，不能要求回退当前500分正确下限。 |
| §7 L103–111 文件布局/根导出 | `config.rs` 与 `lib.rs:24` 已存在；测试布局也在；已实现。 |
| §8 L113–120 验收 | 类型/错误/导出有源码；test/clippy/build未运行。旧ST5%已由当前主板风险警示10%政策取代，lot_size任意正数已被正式100股单位取代，不新报遗漏。 |

## 新候选与反证

### C15-01：Web 的成本舍入与浮动盈亏没有承接 Account 派生契约

- 原文：account spec L23/L61 要求成本“银行家舍入到分”，L63 要求 unrealized_pnl=`(price−cost_price)×qty`，L147–152再次明确纯整数 half-to-even；plan L255/L714 同样约定。
- Rust 当前满足：`account.rs:658` / `:673` 半偶；`:599` 对已舍入成本算每股差再乘股数。
- 实际 Web 路径：`session/snapshot.rs:193` 将 qty/invested/recovered 投影 → `apps/web/src/host/protocol/parse.ts:179` 解析 → Redux snapshot → `apps/web/src/app/portfolio-selector.ts:9` → `LocalRefreshViews.tsx:23` usePortfolio → `:38` 原始 netInvested/qty 得 avgCost、`:40` 市值减未舍入净投入得 pnl → `:137` 持仓成本/盈亏展示。`apps/web/src/utils/format.ts:134` 的 yuan 对金额使用 Number.toFixed(2)，没有半偶处理。
- 合法可达短例：同一买入委托分别100股成交1000分、100股成交1001分；qty=200、invested=200100分、recovered=0、现价1001分。Rust成本1000分（1000.5半偶向偶），unrealized_pnl=200分；Web成本显示10.01元（应按engine为10.00），pnl=100分。此例金额均远低于 safe integer，不属于Q01范围歧义。
- 已执行纯 Node 算术表达式（未启动应用、未跑测试）：输出 `{qty:200,netInvested:200100,currentPrice:1001,webCostYuan:"10.01",engineHalfEvenCostCents:1000,webPnlCents:100,enginePnlCents:200}`。引擎1000分由现有整除/奇偶代码独立手推，未伪称运行Rust测试。
- 反证检查：账户整体serde演进、total_assets移层、T+0取消都没有取消成本half-to-even；现行ADR-0011/0012仍要求权威个人成本。当前总账R07与旧核心复核只核销Rust helper/结算，未登记这条前端消费漂移。需要父审计独立确认并统一“展示净投入精确收益”与“Account.unrealized_pnl”名称/公式，不能单靠改标签或改测试预期宣称行为一致。
- 分类建议：新增跨层派生/显示遗漏候选，金额精度Q01之外。建议代表性短测直接覆盖生产portfolio推导的半分正负成本、非整除、成本与pnl相同输入跨层一致性；本轮不实施修复。

### C15-02：负成本显示 `-` 的历史句子需要明确显示对象

- account spec L25 写“成本为负时显示 `-` 而非 `xx%`”，L105明确交给前端。`LocalRefreshViews.tsx:137` 成本列直接 yuan(avgCost)，负成本实际显示负金额，当前没有成本收益率百分比列。
- 反证：原文同时说“成本价显示”与“而非百分比”，存在对象歧义；ADR-0012 L47–48后续明确的是个人成本收益率不可用，而不是删除负成本金额。当前前端不生成该百分比，不能仅凭负金额认定违反百分比规则。
- 分类建议：文档歧义候选，保留证据，未认定确定代码遗漏；不得强行把Account负成本归零或抹去净投入事实。

## 已有 Q 与被排除的误报

- Q01 保留：`money.rs:42` 仍i64/JSON整数；Web `host/protocol/parse.ts:179` 对invested/recovered用parseMoney；共同金额范围未统一，config仅非负。C15-01的小额半分错位独立于该Q。
- Q06 保留：`session.rs:1090` 权重校验、`:1107`有效正权重和；`:1740`散户eligibility、`:1759`类内tail采样。与旧“和约等于1、类内随机”关系尚需收口。当前指定三篇不新增分配需求；G29零NPC正流通盘矛盾已登记，不重复计数。
- total_assets同名API缺失已反证；apply_trade无人生产调用已反证；Account无serde derive已反证：`session.rs:2562` 保存 cash/Position事实，`:2716` restore先validate_save_slot，`:2735`恢复所有持仓事实；`persistence.rs:759` 校验现金、未知代码、数量、T+1、非负累加器，合法事实的净成本可为负。
- `Position::from_restored_parts`允许构造异常数据和`grant_position`接受零仓属于现有可信构造/统一边界设计；真实外部恢复先校验，不新报可达存档漏洞。旧spec现金示例“1000元+5元=1500分”单位错误不作为实现目标。
- 正式T+1固定、ST10%、100股申报单位、过户费与卖方收费封顶是后续领域规则/已登记游戏简化，不按旧config旋钮或无过户费示例认定代码漏做。

结论：三篇全文覆盖后，核心账户/配置主干未找到新增生产断链；新增 C15-01 跨层成本/盈亏派生候选及 C15-02 文档歧义，交父审计独立复核。没有全量测试通过或绝对无遗漏的声明。
