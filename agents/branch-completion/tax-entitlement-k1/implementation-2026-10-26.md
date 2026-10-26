# TAX-K1 原登记权益 kernel 实施记录

日期：2026-10-26。唯一 writer：`/root/sol61_tax_k1_resume_fresh`，接棒同模型前任。
独占树：`.worktree/tax-entitlement-k1`。接棒 HEAD `c5626e7928ad283e60e0c8b8e5f6780e993e345e`；
完整 K1 diff 基线 `54ceb709`。本记录是工作证据，不替代正式交易规则或生产接线许可。

## 范围与依据

只改 `company/cash_dividend_tax.rs`、`cash_dividend_tax/entitlement_units.rs`、其 `tests.rs`
及本工作目录。无新增依赖；`num_bigint` 为现有依赖。未改 SaveSlot/Web、真实账户、
REG-P/日事实生产者、旧 `outstanding` 的处置口径、旧 Bonus 税基、R2 个人 Bonus Unsupported 门。
只新增只读 `assessment_with_unit_lineage`，其结果尚未用于生产税款划收。
因此不能据本片绿灯宣称生产税 bug 关闭或个人送股功能完成。

遵循已独审的设计 v3：SHA `a1e0e2ed57024c7eedf7685fce65990bfbeacdca5cfbb3e0cf7aaafd59fad761`，
见根树 `agents/branch-completion/personal-stock-bonus-tax-basis-design-2026-10-26.md` 及同目录
`personal-stock-bonus-tax-basis-design-review-2026-10-26.md`。已读 ROLE、AGENTS、principles、
architecture、testing、open-questions、ADR-0038/0039/0040、Git 工作流。
领域依据沿用本轮已取证现行官方原件：财税〔2015〕101号第一至三条、财税〔2012〕85号第三/四/八条、
中国结算2012-11-30通知。已核存储原文相应三档、FIFO、日净额/持期段落；准确 URL、
适用日期、原件 SHA 见根树 `tax-basis-sources/manifest.json` 与 `tax-basis-review-sources/manifest.json`。
101号2015-09-08之后登记范围覆盖旧暂扣口径。这里的区间与缩股分配全部是给定条件算术，
不以手写映射证明真实沪深来源或碎股政策。半份额是内部权益承载，不是可交易半股。

## 实现合同

- 从真实税账操作序重放物理 lots，在每个现金 R 登记时建立派生 carrier；每个 R 保留独立原单位。
- 单位 transition 只绑定真实核减或明确 CorporateAction 正向取得的操作，核对日期、事件、前后 lots、
  predecessor。相同 transition 重试幂等；同身份异事实拒绝；缺核减映射显式拒绝。
- source 端含 transport/retirement、target 端逐 lot 都校验完整、不重叠、容量以内的半开区间 partition。
  处置先消费当前 lot 区间，再通过同一片段的线性比例机械反投影原权益；剩余坐标随 FIFO 消耗平移。
- 内部 exact rational 使用现有精确类型的严格序列化；乘除/比较以 BigUint 中间值避免溢出误判，
  约分后的 numerator/denominator 超出 i128/u64 时返回 typed Overflow，不截断、不用浮点。
- 现金实际已付所得、登记持期、限售与事件合计 half-up 到分抽成共同循环。旧入口保持原处置数量与原累加顺序；
  新入口仅把明确处置的原权益份额代入。已实际收税照扣，旧 collection 回执不改写。
- 原权益退休只留明确 retirement，不虚构处置/补偿/免税/结案；后续新买 lot 不能承接该历史所得。
- typed `(TaxIncomeKind, event_id)` 贯穿本片输出。K1 只处理 Cash；旧新 lot Bonus basis 明确拒绝。
  Cash/Bonus 同名激活、Bonus 原权益与统一激活恢复属于后续条件 K2，未在此宣称覆盖。

物理存活 lot 的税龄仍是旧账既有事实；混合来源的 K1 fixture 只验证已登记现金所得的历史归属，
不能证明转换后 current lot 参与未来 R 的取得事实已正确。受控来源生产者、真实税务日终、
中间 capture、碎股所得和退休终结规则仍须后续来源门与独审。

## TDD 与原始证据

原测试/stub `c5626e79` 尚无有效行为红。前任已修静态预审发现的 fixture 连续日期：
显式给定01-02零净额，再同日核减；第二转换同样保持完整日序。没有削弱旧校验。
接棒保留既有 fixture 修订和新增边界，`be70aaec` 保存实际行为红测试。

| 阶段 | 实际结果 | 原件 |
| --- | --- | --- |
| red-wire-01/02 | 历史 harness 接线错误，非业务红 | evidence 对应日志 |
| red-wire-03 | 仅保留前任空日志及 .tmp 既有 artifact，未冒用为新源红 | .tmp 原件保留 |
| red-wire-04 | 本次首次调用遗漏 CLI `--`，usage 错误，非业务红 | evidence 日志 |
| red-wire-05 | 10000ms 外监督编译超时并终止；非业务红 | evidence 日志与 .tmp before |
| red-wire-06 | 明确300000ms长构建成功；实际运行12个case全红，exit101，0.044s | before/artifact/run JSON、red-behavior-06.log |
| green-wire-01 | 12/12绿，第一轮行为实现 | evidence 日志 |
| green-wire-02/03 | 提取共同评税循环并保旧累加顺序；72/72绿 | evidence 日志/03 receipts |
| green-wire-04 | 77/77绿，包括22个lineage case和真实模块原有55个case；exit0 | evidence receipts、green-all-04.log |

真实红的关键断言：同一真实 book 已付每原股100分、原100股2并1→50后卖尽，旧 `outstanding=1000`，
stub 新入口实际1000而目标2000，失败落在新行为断言；没有把 fixture准备错误算成行为红。
12case红 binary SHA `b22f4f3b486862c7ecfa689cba475ff8a03f040c6b85733f41f7d80ee2072782`。
最终77case绿 binary SHA `1aecf1e0f4dfa645d441e0da369ddef4e744796d5fb2b3339e9f1ab72e0cc32f`。
完整源码/依赖 SHA、真实 argv、run exit、前后无漂移校验、binary大小均在 JSON。

额外覆盖：10/40分次消费20/80原权益、混合长期/短期、内部1/2份额、双端重叠/缺口/越界/
空区间/倒序/未知lot、同总量source重叠、正向扩张到两个真实lots、一源多片段、连续转换、
第二次独立R、仅登记未付/部分实付/晚激活、旧收300后新欠1700、book和lineage恢复后继续40、
strict JSON缺字段/未知字段/不约分/零分母、直接内存坏映射、合法边界分数投影溢出及输入不变。

## 构建与执行边界

轻 harness 直接 symlink 本树真实 domain module（含真实测试）；只逐字摘取边界 StockCode、AccountId、
CompanyId 和现有 serde helper，没有仿写业务库。实际摘取概念与各自 SHA 明列最终 artifact 的
`boundary_extractions`；完整原始源 SHA 列 `files`。仅复用根 `.tmp/company-read-views-target/debug/deps`
现有 rlib；未运行 Cargo、未创建冷 target、未写 engine 生成型。TS export 输出隔离在 `.tmp/.../bindings`。

构建 `codegen-units=32`，外部 `runBoundedCommand(timeoutMs:300000)`；正式测试整命令
`run-with-deadline.mjs 10000 -- python3 .../run-light-harness.py green-wire-04 ''`，
真实 binary `--test-threads=8 --nocapture`。整命令10秒也硬约束其中每个 case，无普通case放宽。
77case binary报告2.48s，含前后绑定校验的 Python wall见run JSON；非完整engine回归。

最终轻构建实测2.546s；50ms进程树采样（14个样本，详artifact）峰 RSS455331840B、
峰23threads，最终 binary25851480B。先前1–8MiB预估被实测纠正：真实module测试和ts-rs使binary约25MiB。
本片只有一个rustc及其linker进程树，与主线大构建共享轻lane；没有无界长单核任务。
CPU tick采样是进程树原始数据，threads峰不冒充实际runnable峰或满核证据。

证据目录 `evidence/manifest.json` 密封各原件 SHA；原始失败日志保留，不回填、不改名冒充通过。
早期 wire 01/02 的单一 binary路径被后来构建替换；其实际当时运行绑定已在run receipt保留。
wire03/04以后每标签独立binary和harness源码留于根 `.tmp/tax-entitlement-k1/`，可以校验后复跑。

## 完成门

本记录截至提交前仅说明实现与有限测试通过。尚待未实施者完整 diff 独审，不能先宣称 K1完成。
指定复核者：`/root/sol61_distributions_r2/personal_bonus_design_review`。


## 初审 F1 修复（2026-10-26）

未实施 reviewer 完整初审为 CHANGES_REQUESTED，仅F1阻断：未映射 CorporateAction 正向不能默认
独立新增。原100股已收10000分，若实际1拆2、未给映射、卖20，原实现会消费20原权益并评400分，
明确完整split映射则消费10原权益并评200分。旧 source 也能表示独立配股，不能猜它是哪一种。
初审其余共同循环、双端partition、操作绑定与严格重放无新增有效阻断；其独立77case重跑通过。
初审正式原件见根树 `agents/branch-completion/tax-entitlement-k1-review-2026-10-26.md`。

- 红：`17853ac0` 新增一个直接旧book正向+100、lineage空、再卖20的拒绝case；
  review-f1-red实际exit101、1红，0.050s；binary SHA
  `df45f37273875948aee91dcd8d27424c4028ac6dbe090e0d2254cd6bab930141`。
- 修：只在新候选重放入口增加fail-closed guard，未附transition的CorporateAction正向统一返回
  `TaxLineageError::MissingTransition { event_id, operation_seq }`。不猜split，不批准独立配股来源，
  不增加受控分类协议；要支持该独立新增须后续来源分类合同。SecondaryMarket独立新增正控仍通过。
- 绿：review-f1-green-02真实78/78绿（23个lineage＋55原case），exit0；binary报告3.44s，
  含绑定校验wall3.486s，8threads，整命令10秒外监督。binary SHA
  `d865ecd7a5889f78c5e92a8209ed4907488798578ea1e71dab0981aa6859c068`。
  构建2.449s、codegen32、300000ms外监督，进程树采样峰425340928B/10threads。
- 中间一次字符串替换因模式不匹配报AssertionError，未修改源码；随后review-f1-green仅编译了
  仍未修的源码且没有运行，不能当绿。修正替换后另用review-f1-green-02新标签真实构建/运行。
  所有标签的.tmp artifact均保留，证据日志不覆盖。

该修复冻结后仍待同一未实施 reviewer 复审终裁。本轮没有实施K2、生产接线、SaveSlot/Web变化。
