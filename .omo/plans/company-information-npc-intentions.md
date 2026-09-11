# company-information-npc-intentions - Work Plan

## TL;DR (For humans)

**What you'll get:** 公司通过真实经营事件形成会计分录、行业财务报表与公开披露；不同NPC结合基本面、量价、技术和个人经历形成自己的判断，以有限资金执行可跨日的个人交易计划。玩家可查看已公布财务信息，开发环境可追踪个体决策。

**Why this approach:** 复用现有独立账户、撮合、注意力、经历及母单，不另建交易系统；以财务勾稽、信息时序、真实成交和确定性重放验收，不靠预画行情证明“拟真”。

**What it will NOT do:** 不给股东分红或派钱，不执行增发/回购/清算分配；不保留旧档兼容或共同正确股价；不强制增加银行/保险/地产股票，不用代表性账户替代独立NPC。

**Effort:** XL。**Risk:** High — 完整会计与多行业、双时钟、逐户状态和跨宿主契约共同变化，必须分批验证。

**Decisions to sanity-check:** 完整支持工商/银行/保险/地产对应报表；公司正常经营与商业债务收付执行，股东结算仅设计；默认2030年开局且日期可选，公历真实、未知休市明确为模拟；个人计划可跨日但委托日终失效；发布产品移除额外调试，本地存档无需保密。这些范围已获用户确认。

**规模：** 六个实施波次，42个实施与测试任务，另有4个最终独立验收任务。所有任务都属于完整交付，不以分波为由缩小范围。

**当前状态：** 仅计划落盘。已经完成Metis缺口分析并据此修订；未执行产品代码、脚手架命令、编译或测试，未进行可选的高精度双重复核。

**Your next move:** 在独立工作会话执行，或先要求高精度计划复核；规划会话不自行开始实施。

---

> TL;DR (machine): XL / high risk / 42 implementation tasks + F1–F4 / approved scope / planning-only / all implementation verification pending.

## Scope

### 授权、决策优先级与交付性质

- 用户已明确批准本方案并要求落盘。本文只规划，所有产品实施均待独立 worker 会话启动。
- 用户本轮已确认决定 > 本计划固定契约 > ADR-0016 原 proposed 的候选范围。与已接受 ADR 冲突之处先由任务2更新决策记录；不得只改代码不改语义。
- 完整交付包含公司经营/会计、工商/银行/保险/地产报表、公开披露、个体混合分析、持续交易计划、关注与预算、三宿主/UI、存档和量价诊断。分波执行不代表 MVP 或可少做。
- 本文是机制实施计划，不是“已证明符合真实 A 股统计分布”的报告，也不声称完整覆盖所有会计准则及金融合同。所有支持的会计业务均须有实际分录、报表分类和验收；不支持的合同/业务显式拒绝，不能输出假的完整报表。
- 文件路径均以仓库根目录为基准。标注“新增”的路径是计划交付，不是现存文件。行号取自规划时观察，worker 用符号定位。
- 规划工具限制：没有 Shell，未运行脚手架、Git、编译或测试；本文经文件编辑工具落盘，模板结构来自已读取脚手架，但不是脚手架生成。不得宣称这些命令已执行。高精度双重复核未请求、未运行。

### Must have

| 需求 | 任务 | 验收主证据 |
|---|---|---|
| 真实公历、2030默认起点、自然日/交易日双时钟 | 4–5、28、34、37 | calendar、session、三宿主回放 |
| 完整复式记账及五类报表产物 | 6–14 | accounting、industry_reports、consolidation |
| 工商/银行/保险/地产差异，不强加默认股票 | 8–13 | 四类测试实体+默认股票集合不变 |
| 公司经营资金与投资者资金隔离 | 7–14、28 | 外部对手方账、禁止股东支付对照 |
| 经营事实/披露/个人获知分离 | 15–16、28、33 | information、无前视场景 |
| 无共同 V，混合基本面/技术分析 | 17–19、26 | beliefs、strategy、ComputeBackend |
| 个人经历、持续计划、紧迫度、跨股选择 | 20–25 | experience、plans、execution |
| 新格式存档，删除旧格式专用代码 | 27、29–32、41 | save_contract、原子恢复、删除清单 |
| 公开财务展示和dev专用调试 | 29–35、40 | company-information E2E、release feature矩阵 |
| 基线/统计/规模/确定性 | 1、28、36–40、42 | 多seed报告+跨宿主+scale |
| 每批独立A股语义/必要性复核 | 每任务及F1–F4 | diff审查与修复复核回执 |

### Must NOT have (guardrails, anti-slop, scope boundaries)

1. 不保留共同正确 V、均值回复价轨道、全市场目标价；不把 V 改名为“公司价值”后继续共享给策略。
2. 不伪造订单、成交、成交量、盘口承接或强制跌后反弹；公司事件只经本人判断/合法订单影响价格。
3. 不合并 NPC，不共享个人账户、成本、记忆、注意力或 RNG；2万/5万/10万账户门禁保持。
4. 不执行分红、股权融资、回购、股东清算分配；不增减股本、不进行除权除息参考价处理、不向投资者补钱。相关结算设计仅文档，无运行时待执行队列、API或定时器。
5. 不混淆公司商业借款与股权融资。公司经营收付款、商业借款/还款/利息/税费必须执行并记账；这些不是给 NPC 派钱。
6. 不新增默认银行/保险/地产股票。保持默认证券代码、交易所、证券类别和初始股本集合；可为其明确虚构经营行业并增加独立测试公司。
7. 不纳入停复牌、退市交易机制、ETF/套利、新板块、市价申报扩展、内幕交易/传闻玩法或整个国民经济仿真。
8. 不保留旧档迁移器、旧引擎或旧格式特判；不删除用户文件，不弱化当前格式有效性、T+1/费用/撮合断言。
9. 不做本地存档保密、加密、反作弊；dev/release差异限额外诊断，不能改变经济行为。
10. 不新增在线数据/LLM依赖，不依赖墙钟、UI刷新或线程完成顺序。保留CPU权威执行，不趁机实现GPU。
11. 不给未校准概率/税率/会计假设贴“真实A股参数”标签；未知官方规则必须作为证据阻塞，不用印象或随机默认值替代。
12. 不全仓库重构。只抽取本次触及责任；新文件控制在250纯逻辑行以内，测试按场景拆分；大文件旧内容的迁移必须由原行为测试保护。

### 固定实现契约（所有任务共同引用）

#### K1 — 时钟、日历与规则身份

- 新增 `CivilDate`（ISO YYYY-MM-DD）、`CivilInstant`（Asia/Shanghai日期+日内秒）、`TradingDayOrdinal`、既有 `MarketMinute` 分离类型。金额/日期/交易分钟不混用。
- 默认 `start_date=2030-01-01`，支持2000-01-01至2099-12-31开局和推进；超范围明确错误且不改变状态，不循环年份。配置入口为新游戏日期选择和 `SessionSetup`，各宿主一致。
- 初始化专用公历/交易日历/规则覆盖向前延伸至1998-01-01，供2000年最早开局的2年财务及360交易日K线前史；不开放1998–1999作为运行开局选择。缺正式覆盖的历史和未来分别标记`SimulatedHistorical`/`SimulatedFuture`，不把历史缺资料说成未来未公布。
- 日期严格按公历（含2000闰年、2100不闰的纯日期算法测试）；交易会话时间继续复用现行阶段和标准交易分钟。自然日跳转不能伪造240个交易分钟。
- 休市起点保持实际起点，不静默挪到开市日。一次无交易日推进处理当日到期经营/披露再前进到次日；不产生行情tick/成交，也不唤醒市场时间注意力。交易日仍按原tick顺序推进，收盘后补齐当天经营及18:00披露，到次日才更新civil date。
- `CalendarPolicy` 包含算法版本、适用年月范围、沪深分别的已知休市覆盖区间/日期/原文出处、模拟假日数据和digest。完整政策数据或足以无联网重建的版本化表进入存档；恢复优先用存档，不读最新版默认表覆盖。
- 已有官方交易所年度安排优先；没有公告覆盖的年份按上条区分历史/未来模拟。模拟规则固定为周末休市、元旦1日、劳动节5月1–2日、国庆10月1–3日、春节除夕至正月初三、清明当日、端午当日、中秋当日休市；不模拟补班/补休，也不声称预测正式长假或还原缺资料的历史假期。农历/节气日期来自带来源与摘要的1998–2099离线事实表，不引入运行时网络。模拟规则属于游戏日历假设，不是交易所公告。
- 市场工作日不随政府调休周末变为交易日；真实公告可覆盖默认工作日/假日结果。自然日披露无需顺延到交易日，不以周末为由超法定期限。
- `RegulationProfile` 独立于模拟年份，固定本发布已核验规则和已公布生效安排。2030默认采用届时已明确生效的2026版列报安排；早期开局按已冻结的适用规则，不凭空预测未来修法。源文缺失则阻塞相关法定映射。
- 合同使用实际自然日/合同约定basis，默认测试和虚构合同 ACT/365F、固定利率；不宣称所有真实合同如此。到期遇非交易日，公司收付照合同结算日历执行，不偷用股票交易日顺延。

#### K2 — 公司、会计和资金边界

- `CompanyId` 与 `StockCode` 独立；发行人映射、主营行业 `IndustryId` 与证券类别分开。`CompanyKind = Industrial | Bank | Insurance | RealEstate`；允许未上市测试实体及固定集团关系。
- 公司 `AccountingState` 包括科目表版本、借贷分录、期间、余额索引、合同/资产/库存子账、外部对手方、税务子账、结账状态。投资者 `Account` 不复用为公司经营账户。
- Journal为权威来源；每个`BusinessEventId`至多入账一次（重复返回显式重复错误，不重记），整批先验证后原子提交。分录有来源ID/日期/业务种类/借贷行/现金流类别；每行正金额，方向由借贷表示；负权益、亏损允许，负现金/数量不靠clamp掩盖。
- 新 `AccountingAmount` 使用有检查的i128分、跨JSON有损风险一律十进制字符串；与市场Money转换须范围检查。比例使用整数基点，利息小数保留合同累计余数，付息/报表落分采用固定半偶舍入并记录尾差，不对公司金额转JS Number。
- 开局实体由显式平衡的OpeningBalance凭证与子账构成，不用最新股价或旧V反推财务。公司市场价不直接决定资产/主营利润；公司持有其他金融资产的计量另按已支持合同分类处理。
- 初始财务包覆盖开局前2个完整自然年度和当年截至开局前日，使用独立初始化RNG和同一会计处理器生成经营历史；历史报表仅在其历史公布日早于开局时加入已发布集合，标记`SeededPrehistory`。不捏造实盘历史、审计意见或NPC曾经阅读的经历。
- 现有360根预置K线保持`SyntheticPrehistory`语义并映射到真实交易日；不补逐笔成交、不拿其成交额做对账、不强行让财务与合成K线拟合。游戏开始后的价量一律撮合。
- 外部客户/供应商/雇员/税务机关/贷款人有稳定ID、明确合同余额/收付记录，可是公司域商业对手方，不必都是证券NPC。记录资金跨模拟边界的来源去向，不能假装封闭守恒或给外部对手方无限信用兜底。
- 公司预算包括现金、到期应付、授信额度与抵押/信用条件。资金不足生成`PaymentFailed/Overdue`业务状态和公开风险材料；不允许透支、无条件续贷、资金不足直接引擎崩溃或自动派钱。借款/新增客户合同必须经额度/需求约束。违约不自动触发股东清算、交易停牌或退市。
- 固定股本和初始股东权益需与股票总股本一致，流通股与总股本不能互换。初始资本不是本次运行时发行；利润进入留存收益，但不得自动分配给投资者。

#### K3 — 会计覆盖和行业模型

| 类型 | 必须实现的经营及会计事件 | 特有报表/子账 |
|---|---|---|
| 工商 | 采购现付/赊购、生产领料与完工、销售履约/应收/回款、人员/管理/销售/研发费用、资本开支、直线折旧摊销、坏账/存货及资产减值、借款利息税费 | 存货数量与成本（移动加权平均）、账龄、固定资产、收入/成本与经营现金对照 |
| 银行 | 外部客户存入/提取存款、发放/收回贷款、按实际利率应计利息/结息、费用收入、贷款逾期、分阶段预期信用损失与核销/回收、准备现金约束 | 客户存款负债、贷款总额/减值准备、净利息收入、信用风险阶段及现金流分类；不能把存款当收入或贷款发放当费用 |
| 保险 | 合同分组、保费收取/应收、履约现金流估计、贴现/风险调整、合同服务边际及服务释放、赔案发生/支付、亏损合同组、期末重估 | 保险合同资产/负债、保险服务收入/费用、保险财务损益、CSM/赔付/现金调节；不能把所有保费立即计收入 |
| 地产 | 土地/项目取得、开发成本与符合条件借款费用资本化、预售收款、履约/交付与收入成本确认、尾款回收、存货减值、项目借款偿付 | 开发存货、合同负债、项目进度/交付和销售收款；预售不是交付收入 |

- 所有类型都输出资产负债表、利润表、现金流量表、所有者权益变动表、必要附注；各有真实科目映射，不强套工商模板。银行/保险/地产没有默认股票也必须可通过自定义CompanySpec和公共报告查询跑通。
- 银行支持固定利率、摊余成本、明确合同现金流的存贷款及三阶段ECL（PD/LGD/EAD情景输入）；保险支持一般计量模型的明确期限非分红合同，包含盈利/亏损组与CSM；地产支持在交付时转移控制权的开发销售。结构化衍生品、再保险、投连/分红合同及其他计量模式必须显式`UnsupportedContract`，不得冒充已实现；这些边界不免除所支持模式的完整会计及报表。
- 税务政策是有版本的显式配置：应交/递延所得税、可抵扣亏损、适用业务销项/进项税额分开；不可抵扣成本按政策归集。生产默认值须任务2核验现行适用法源；单测可使用标为Fixture的合成税率。不得用“利润×一个税率”声称覆盖全部税务业务。
- 合并支持固定母子公司关系、控制范围、持股比例、少数股东、内部往来/交易/未实现利润抵销；开局后不做并购/股权买卖。单体与合并分别标记，不双计现金。没有子公司时返回明确NotApplicable，不伪装合并。
- 每日`finalize_day`只标记当日due业务处理完毕，不封死全年总账。月末封月、年末结转；季报/半年报是已定稿业务范围的季度/YTD不可变快照，不阻止后续月份记账。已封月份只能通过带更正来源的调整凭证在当前开放期间入账；重述历史报告使用独立restatement工作底稿，把调整映射到相关历史期间，保留原凭证/原公开版不变，防止重记现金。
- 期末结账产生TrialBalance和不可变报告版本，现金表直接法及净利润到经营现金的间接调节，权益期初/净利/OCI/期末对账；上期比较可以是合法`Unavailable(reason)`并允许发布，不能填0或省略必要原因。更正以新凭证/新发布版本表达，旧发布版不覆盖。
- 每种适用准则的确认/计量/分类须任务2官方来源门禁；完整覆盖本表业务，但不作“所有中国准则完全合规”声明。需更多科目时从已定义业务推导，禁止随机填报表。

#### K4 — 经营演化、披露与信息

- 每公司使用独立经营RNG，市场/行业冲击另有独立流；以seed+稳定ID派生并持久化其状态，不共享策略RNG。
- 按自然日推进需求/订单/交付/生产能力/成本/回款/授信；原始订单、交付与收款事件驱动会计。持久合同到期事件队列代替每tick扫描完整账本。
- 起始公司业务预算使用明确配置；实际销售不能超过已接受需求与库存/服务能力。最低默认事件目录：市场需求扩张/收缩、行业成本变化、单公司合同取得/取消、客户延付/信用恶化、生产中断/恢复、资产减值迹象。效果是需求/成本/履约/风险参数变化，不是价格百分比。
- 默认游戏冲击参数：市场/行业每天独立1%候选概率，公司2%；影响持续5–30自然日，幅度基点区间写入配置并版本化。先用±500bp市场需求、±1000bp行业成本、±2000bp公司需求；压力场景单独显式配置。均属待校准假设，C02敏感性分别用0.5x/1x/2x测试，不能声称现实频率。
- 定期报告：自然年会计年度；年报计划次年3月20日18:00、Q1为4月20日18:00、半年8月15日18:00、Q3为10月20日18:00，各公司稳定偏移0–7自然日。这些是游戏排期，须在任务2核验法定窗口且Q1不早于上一年年报；合法公告可在休市日发布，不推迟到开盘。
- 日终先finalize当日业务，若月/年末才进行对应封账，再执行18:00披露；临时重大经营事件形成结构化公告，按发生后的下一个18:00发布，内容只含该时点已确认事实。未来合同现金流是带不确定性预测，不可标已实现。
- `PublishedReport`含公司、范围（单体/合并）、期间、会计政策、批准/发布时点、版本、来源/更正关系、报表/附注；不可变并可按ID查询。普通客户端只收公开版本，不收未披露总账。
- `NpcInformationState`逐公司记录获得的报告/公告ID及观察时间；公共曝光仅改变发现机会。未观察NPC不能直接读最新全部公告；借助公共索引定位候选后，只有实际观察才将内容纳入个人信息集。
- 观察是在现有候选注意力被接受时发生；休市日不增加市场分钟或偷偷全体阅读。dev查看未公开数据不得写入NPC信息状态。
- 发布历史与被个人状态引用的版本不能删除；已关闭日记账可按月归档为有序无损块/索引，不丢凭证，不要求每tick克隆历史。新报告/更正不会追溯重写玩家或NPC当时看过的材料。

#### K5 — 个体分析、记忆、预期

- 账户身份、稳定风格、分析能力/权重、信息获取、预期状态分开。保留现有ordinal→账户身份/主导风格映射，新局独立派生参数；机构不强制读基本面，散户可分析基本面。
- `AnalysisProfile`具有基本面/趋势/量价/技术/成本经历五个非负权重及分析方法；权重总和10000bp。不通过每次观察随机切人格。
- 默认仅作为显式游戏分布，以下顺序均为基本面/趋势/量价/技术/成本经历：LongTerm 5000/1000/500/1000/2500；DipBuyer 1500/1000/1000/1500/5000；Momentum 500/4500/2000/2000/1000；Noise 0/1000/1000/1000/7000；Panic 0/2000/1000/1000/6000；Dormant 1000/1000/500/500/7000；机构DeepValue 7000/500/500/500/1500、Growth 7000/1000/500/500/1000、Balanced 6000/1000/1000/1000/1000、Defensive 6500/500/500/500/2000、ActiveTrader 0/4500/2500/2000/1000；游资Momentum 0/5000/2500/2000/500、Reversal 0/1500/2500/3000/3000。实例各非零权重一次采样0.6–1.4倍率后最大余数法整数归一，字段顺序破同分，零权重保持零。
- 基本面可用时，个人预期包含盈利/增长/现金转换/风险预测、期限、信心、所用报告ID及方法；估值为`Unavailable(reason)`或个人区间，不设置市场正确目标。
- 方法至少三条实质不同路径：工商/地产的个人现金流折现（5年预测+有条件终值，资本成本必须大于终值增长，否则该法不可用）、可持续盈利倍数法（负或不可持续盈利时不可用）、银行/保险的账面权益与可持续ROE/资本成本分析。公式基于本人已知报表及个人假设，不能共享一个估值后再加噪声。
- 初始先验从本人实际选择阅读的已公开历史报告生成；无报告则无估值。经验/信心调整基本面输入的个人预测，不修改公司事实。更新仅由新获知材料、既有预期到期、本人经历/风险变化触发，不每tick抖动。
- 信心0–10000bp、估值期限按主导风格5/20/60交易日；长期/价值60、均衡/成长20、其他5。信心只是行动权重，不是统计置信区间。负权益、亏损、零收入分母、无限估值必须显式不可用/高风险处理，不抛弃整个NPC也不补正值。
- 技术信号复用具名市场窗口与真实已发生行情：原30分钟/5日/区间/量能/失衡继续有效，新增SMA20/SMA60、RSI14和ATR14按完整日K计算；历史不足带可用长度/原因，不能补趋势。技术观察都是可获得信息，个人是否观察和记住的参考点独立。
- 个人价格记忆上限复用持仓+8个未持仓股票，保存首次/最近观察价、已观察高低、时间窗口、来源；不能把没看过的过去价格说成亲历。专业技术分析可主动读取公开历史，但记忆要记录本次读取行为。
- 复用失败买入、成本、浮盈回吐、冷静期和账户峰值。新经历状态为忍耐/信心/风险压力及失败事件日期；日历时间与市场分钟分开。实际买入失败仍由成交后本人观察的不利变动确认，部分成交同订单去重。每20交易日无新受挫时失败影响减弱一档（不是删除真实亏损）；清仓保留账户经历，120市场分钟冷静期保持，长期被套以持有满20交易日且低于成本的本人观察判定。

#### K5a — 分析到计划的固定数学契约（待校准游戏参数）

- 所有信号统一`SignalScore`∈[-10000,10000]。定义`N(x,t)=round_even(10000*x/t)`再数学截断至上述区间；t必须>0，x和t单位相同。这里的截断是明确定义的评分函数，不是修复非法状态的fallback。未知信号是`Unavailable(reason)`，不输入0假装中性。
- 基本面：现价落个人估值区间内score=0；区间外用个人区间中点相对现价的折溢价bp作为x、t=2000bp作N。现金流/盈利/权益法结果各属于本人，不能先求市场平均估值。
- 趋势：`0.4*N(30分钟收益bp,300)+0.6*N(5日收益bp,1000)`；两窗均需合法完整，否则只对有样本子项按原子权重归一并记录不可用项；两项均无则Unavailable。
- 量价：`0.5*sign(30分钟收益)*N(相对量能bp-10000,10000)+0.5*盘口失衡score`；每个子项有各自样本要求，缺失处理同上。相对量能使用原观测分母，不发生成交。
- 技术：`0.7*N((SMA20/SMA60-1)的bp,500)+0.3*N(50-RSI14,20)`；ATR只供风险/执行，不冒充方向。RSI全部平价且14个有效样本时定义50；缺样本不可伪装50。
- 成本经历：复用原零基本面behavior候选判断，将Hold/Watch/TryBuy/Add/Reduce/Exit分别映射0/0/2500/5000/-5000/-10000，原理由和风险约束保留；调用该候选内核不得递归调用混合聚合器。
- 综合判断`S=round_even(sum(weight_i*score_i)/sum(available_weight_i))`。不可用/零权重项排除并记录，所用权重总量可追踪；全不可用时禁止新方向计划，返回Watch/InsufficientInformation，已有计划保持或按期限/约束暂停，不静默造0分卖出。
- 主导风格对应基本面方法：银行/保险用权益ROE法；工商/地产DeepValue、Defensive用盈利倍数法，Growth用现金流法，其他有基本面权重者按稳定AccountId奇偶选盈利/现金流，选择只在工厂发生。方法不适用返回明确Unavailable，不无声换另一模型。
- 个人预测输入取最近本人已知完整年报及可用中期更新；增长初值取本人已知最近2个年度可比收入增长，限制在[-2000,2000]bp作为预测先验范围，再加一次性个人偏差[-1000,1000]bp。不足2年时增长假设显式`PriorWithoutHistory`=0加个人偏差，不称历史事实，报告仍可估值但初始信心降至3000bp；有2年信心6000bp。
- 盈利法：可持续利润=本人已知最近完整年度净利扣明确非经常项目；个人质量系数初始8000–12000bp，PE个人值DeepValue8–16、Defensive6–12、其他10–24（整数），按独立profile RNG一次取值；净利<=0则该方法不可用，不取绝对值。
- 现金流法：起点为本人已知年度经营CF减资本开支加新借款本金减归还借款本金；若经营CF未扣利息支付，再扣对应利息支付以统一口径（报告明确为股东可分配现金流预测，不实际分配）。使用与权益现金流匹配的权益资本成本，5年按个人增长预测；资本成本800–1600bp一次取值，终值增长0–300bp且必须<资本成本；任何口径无法从报表确定时Unavailable，不混用企业自由CF与权益折现率。
- 权益ROE法：使用本人已知归母权益及归母可持续净利、显式平均权益口径；预期ROE=观察ROE加个人[-300,300]bp偏差；个人资本成本800–1600bp，账面权益乘预期ROE/资本成本形成个人估计，权益或预期ROE非正时Unavailable/高风险。持有公司类型不同不能换成固定PE。
- 三种方法先计算**归属于发行人普通股股东的整体权益估计**，必须使用一致报告范围；合并场景剔除少数股东对应权益/收益/现金流，不把集团全部权益混作归母。每个情景再除以发行人固定的**已发行普通股总股数**，转换为每股估值分值，绝不除以流通股数。比较报价前验证报告/发行人/股数关联、正分母、货币单位、检查除法与半偶舍入；个人估值区间存每股价格，不把公司总价值直接与股价比较。
- 区间为方法的悲观/乐观两个情景：个人增长±300bp、质量系数±1000bp或ROE±200bp、资本成本±200bp；每场景各自验证有效性，任一无定义则返回方法不足以给合法区间的原因，不补正值。全部中间运算用检查定点/有理数，同一半偶舍入函数。
- 新信息修订预测使用`new=round_even((1-lambda)*old+lambda*observed)`，lambda按能力中心LongTerm/Value=4000bp、Growth=6000、其他=2500；离散信用违约/会计更正允许直接重估，原因记录事件ID。失败经历信心-1000、真实获利退出+500，界限0–10000按明示评分定义；同一订单/事件只更新一次。
- 非风险交易仅S>=2000产生买向、S<=-2000产生卖向，中间区间不开新方向。目标权重=当前权重+`S/10000 * max_stock_fraction/4`并限制在[0,max_stock_fraction]，再由K6账户预算/现金保留裁剪；数量转换带可用性/舍入原因。风险分支优先且保持原风格纪律，不因综合正分而忽略T+1/账户压力。
- 活跃计划保存上次分析fingerprint（report/记忆/账户约束/价位）和S。重复相同输入不修订目标；新事实出现时只有|S_new-S_old|>=1000、价格相对上次判断变化>=200bp、可用现金/库存/T+1状态变化、风险分支触发或期限届满之一才复核。相反方向必须越过另一侧2000门槛，不在零附近来回翻单。
- 执行紧迫度：买计划在30分钟收益<=-300bp且1分钟收益<=-75bp时暂停并请求撤单；缺任一窗口则该触发器Unavailable而非触发。未暂停时，风险减仓且账户回撤>=2000bp或计划剩余期限<=1交易日为Urgent；无急迫条件且信心<4000或LongTerm/DeepValue为Patient，其余Normal。恢复须下一本人观察，急跌条件解除且S>=2000；停止看好则按计划修订/终止处理，不自动补单。
- 所有上述参数进入明确policy配置并随save固定；敏感性只改变声明的参数组，禁止调单一seed。数学金样覆盖基本面与技术冲突、不可用项重权、全不可用、评分中性、迟滞与重复观察，任务17–18/22–23/26必须实际断言数值。

#### K6 — 个人计划、预算与真实执行

- `TradingPlan`按账户+股票唯一，包含稳定PlanId、观点/来源、目标仓位或股数、累计真实成交、创建/有效期、复核条件、信心、紧迫度、状态和终止原因。状态`Active/Paused/Completed/Terminated`，修订用版本和原因，不把`Completed`用于到期/无资金/撤单。
- 平静且信息/风险/约束无变化时，保持方向和累计进度；不在每观察时重抽买卖。个人期限按风格5/20/60交易日，纯日内风格可当日结束，参数固定可测试。
- Plan不冻结资产；账户内分配器计算尚未报单的软预算，已报委托仍由原冻结系统唯一负责。可用资金=权威cash扣冻结及费用保留；跨股软预算总和不得超过可用资金。现金保留默认10%权益，风险压力时提高至30%，明确游戏参数；单股风险上限沿用已有规则。
- 优先级：持仓风险退出/减仓 > 已有计划续行 > 新机会，同行按信心降序、PlanId稳定破同分。仅比较本人持仓/关注/本轮发现集合，不免费扫描未关注全市场。换股卖单未成交前不得预支其预计收入；成交后下次分配才释放新预算。
- 复用PositionDecision期望/可执行差额，T+1锁定仍保留减仓理由，执行差额为0；卖零股语义与现有路由一致。不得整手化时扩大Exit或超目标买入。
- 紧迫度`Patient/Normal/Urgent`独立于估值：耐心按同侧报价等待；Normal按有限价保护的对侧可成交价试探；Urgent只使用当前已支持的受保护订单语义，不增加交易类型或绕过涨跌停。价格需合法tick/区间校验。
- 30分钟跌势加速、风险压力或被动不利选择可使仍看多者暂停/撤买单；撤单通过原订单API，修改价格等同撤旧报新，丢失原排队优先权。不可撤阶段保留旧单并抑制冲突新单，记录PendingReconsideration而非伪造撤单。
- `ParentOrderPlan`演进为复用执行子状态，普通散户小单可用同执行协调器但不复制机构复杂度；每账户每股至多一个活跃子单，旧生命周期到期通过同一路由。实成交才更新filled，撤单/拒单/未成交不推进。
- 日终子单失效并释放原冻结，个人计划可保留；次日本人下一次观察复核后续发，不能开盘自动复活陈旧订单。部分成交、跨日剩余不足一手、到期、反向修订和终止都有显式原因。

#### K7 — 新格式、宿主与调试

- 新格式只有一套必填严格schema，不添加迁移器或旧格式识别支路；缺失/多余/非法字段走当前通用校验。保存setup的`simulation_policy_id`与完整权威状态，不为兼容重引入废弃`schema_version`。
- 权威新增公司/日历/已发布版本/个人信息/预期/记忆/计划/RNG；可推导预算与索引确定性重建。当前格式恢复先全部校验后原子替换，失败保留会话和源文件。
- 对外`PublicCompanySnapshot`只含已公布信息、版本摘要与当前可查询索引；分页查询公司报表走统一EngineHost能力，REST/Worker/Tauri只是传输。页大小20，最大100，以稳定报告ID游标，不以墙钟排序。
- `CivilDateAdvanced/CompanyDisclosurePublished`是权威更新事件；没有成交也要更新runtime snapshot/public company revision。baseline、delta、seq覆盖和发布压缩规则按ADR0010保持。迟到旧session查询响应不得覆盖新局。
- 开发debug使用编译期隔离的trace sink，权威原因字段可存在但完整payload、缓冲区、接口只在debug构建；独立优化诊断feature `simulation-diagnostics`可用于离线测试/报告，不加入产品默认feature或分发产物。release正常错误展示保留。
- 保存导出允许包含私有状态，不加密；产品普通行情不批量广播2万NPC私有明细。dev仅查询选定NPC的最近128条trace，不全量无限缓存。
- 资源检查沿用server边界并扩展新增维度：同档总解码字节默认512MiB、公司数上限256、股数/账户数沿用现有上限；任何新增可增长集合须在restore前检查长度和整数溢出。512MiB是可配置部署上限，不是10万规模完成的替代，规模测试必须报告实际值和拒绝情况，不静默截断状态。

## Verification strategy

> 所有验收由agent执行；人工不承担测试步骤。最终结果交用户确认不等于让用户代跑QA。

- **TDD**：先增加目标行为失败测试，确认非零测试数和正确失败原因，再实现、回归、必要重构；不能删弱原有效断言。每任务同时包含实现+测试。
- 每任务的`QA happy/failure`都必须运行；新增测试文件/命令是未来worker交付，不表示现在已存在。
- 证据目录统一 `.omo/evidence/company-information-npc-intentions/`（简称`E/`）。每任务写`task-N-happy.txt`、`task-N-failure.txt`、`task-N-review.md`，UI附PNG和Playwright trace，报告附JSON；日志包含命令、环境、预期/实际退出码、实际测试数与断言摘要。成功总门禁必须exit0；TDD红灯及故意非法输入的子进程应以预定非零码和正确失败原因验收，不能要求它们全部exit0。不得用源码grep作为行为通过证明。
- 单元：cargo test，既有Node/TS测试框架；集成：真实GameSession/订单簿/serde/宿主actor；E2E：现有Playwright production preview及新增跨宿主驱动器。
- 会计数值金样：单位元便于阅读、执行用分。期初cash/equity=1000；借500；现金收入200；现金费用80；计提利息10、Fixture税22（未付）。预期cash1620、负债532、权益1088、净利88、经营CF120、筹资CF500。另设赊销/税支付/折旧、四行业独立金样，避免仅借贷平衡就算正确。
- 统计不设预定K线路径，不要求所有事件都有分歧/反弹/非零成交；机制不变量硬失败，统计变化完整报告。未经真实数据校准的门槛不能被称为实证拟真。
- 现有总门禁：`pnpm test`、`pnpm lint`、`pnpm types:check`、`pnpm build`、`pnpm test:e2e`；新增显式诊断全套`cargo test -p engine --features simulation-diagnostics`及无feature的`cargo test -p engine --release --test diagnostic_absence`。WASM先运行Windows `scripts\wasm-build.bat` 或Linux `./scripts/wasm-build.sh`；不能只改Rust不更新WASM产物便测试前端。
- Before证据固定任务1的变更前revision及dirty内容摘要；after/parity/scale/release固定最终实施revision。逐批review记录base/head或diff摘要，最终review覆盖完整实施diff；仅新增证据/计划文档的提交不递归使已测试实施revision失效。不得要求before报告来自新实现revision。
- **每批独立复核**：任务实施者不得兼任审查者；审查完整diff、A股制度依据、必要性、边界/跨层漂移、复杂度。发现修复后再审，未过不能关闭任务。保留规范必要的诊断错误而非静默fallback。
- **实际状态**：规划阶段没有执行测试/构建/压力门禁，没有当前通过证据；Metis只是计划缺口分析。

## Execution strategy

### Parallel execution waves

| 波次 | 任务 | 交付 | 并行边界 |
|---|---|---|---|
| W1 | 1–7 | before证据、规则/契约、模块接缝、日历/会计底座 | 任务1最先；2后4/6可并行，3独占旧核心文件 |
| W2 | 8–14 | 四行业、合并、结账报表、经营事件 | 8先完成共享税务/资产会计；9/10/11并行，12/13/14按依赖汇合 |
| W3 | 15–21 | 披露、个人获知、分析/记忆、持续计划 | 17/19/20/21可分模块，18等待披露与profiles |
| W4 | 22–28 | 预算、紧迫度、执行/关注、删除V、恢复和场景 | 22/23/25分模块；24/26/27依序整合 |
| W5 | 29–35 | 共用协议、WASM/Server/Tauri、公共UI、开发调试 | 30/31/32分宿主；共享TS生成由29单一owner |
| W6 | 36–42 | 诊断、跨宿主、敏感性、规模、release、文档、全门禁 | 37/38/39分资源运行，40独占发布目录，42最后 |

本计划内同一个session.rs/strategy.rs/共享生成目录同一时刻一个owner；不能让并行分支互相覆盖。开启前先读工作树状态并保存用户已有改动清单；禁止reset/clean/stash掉用户工作。依赖未完成可写独立纯模块测试，不能声称联调完成。

### Dependency matrix

Blocks列列出直接下游；所有1–41都还必须完成后才能运行42，这条共同收口边不在每行重复展开。

| Todo | Depends on | Blocks | Can parallelize with |
|---|---|---|---|
| 1 | — | 2,3,38 | — |
| 2 | 1 | 4,6,7,17 | 3（仅纯抽取） |
| 3 | 1 | 5,21,26 | 2,4,6 |
| 4 | 2 | 5,7 | 6 |
| 5 | 3,4 | 14,15,28 | 6,7 |
| 6 | 2 | 7,8,9,10,11 | 4,5 |
| 7 | 2,4,6 | 8,9,10,11,12,17 | 5 |
| 8 | 6,7 | 9,10,11,12,13,14 | — |
| 9 | 6,7,8 | 12,13,14 | 10,11 |
| 10 | 6,7,8 | 12,13,14 | 9,11 |
| 11 | 6,7,8 | 12,13,14 | 9,10 |
| 12 | 7,8,9,10,11 | 13 | — |
| 13 | 8,9,10,11,12 | 15,18 | 14 |
| 14 | 5,8,9,10,11 | 15,26 | 12,13 |
| 15 | 5,13,14 | 16,29 | 17,19,20,21 |
| 16 | 15 | 18,25,26 | 17,19,20,21 |
| 17 | 2,7 | 18,19,20,26 | 15,16,21 |
| 18 | 13,16,17 | 22,23,26 | 19,20,21 |
| 19 | 17 | 22,23,25,26 | 18,20,21 |
| 20 | 17 | 22,23,25,26 | 18,19,21 |
| 21 | 3 | 22,23,24,25 | 15–20 |
| 22 | 18,19,20,21 | 24,26 | 23,25 |
| 23 | 18,19,20,21 | 24,26 | 22,25 |
| 24 | 21,22,23 | 26 | 25 |
| 25 | 16,19,20,21 | 26 | 22,23,24 |
| 26 | 3,14,16,17,18,19,20,24,25 | 27,28,29 | — |
| 27 | 26 | 28,29,30,31,32,39 | — |
| 28 | 5,26,27 | 36,37 | 29 |
| 29 | 15,26,27 | 30,31,32,33 | 28 |
| 30 | 27,29 | 34,35,37,40 | 31,32,33 |
| 31 | 27,29 | 35,37,40 | 30,32,33 |
| 32 | 27,29 | 35,37,40 | 30,31,33 |
| 33 | 29 | 34,35 | 30,31,32 |
| 34 | 30,33 | 37,40,41 | 35 |
| 35 | 30,31,32,33 | 36,37,40 | 34 |
| 36 | 28,35 | 38,39,42 | 37 |
| 37 | 28,30,31,32,34,35 | 42 | 38,39（资源隔离） |
| 38 | 1,36 | 41,42 | 37,39 |
| 39 | 27,36 | 40,41,42 | 37,38 |
| 40 | 30,31,32,34,35,39 | 42 | 41 |
| 41 | 34,38,39 | 42 | 40 |
| 42 | 1–41 | F1–F4 | — |

## Todos

> 每任务继承K1–K7及共同QA/独立复核要求；以下路径新增均在实施时创建。本次规划不创建产品文件。

### W1 — 基线、规则与基础状态

- [x] 1. 固定工作树与改动前多seed基线
  - 实施：新增`packages/engine/examples/baseline_fixture.rs`生成当前合法setup输入，新增`scripts/simulation/baseline-run.mjs`运行并收集证据；在任何schema/策略变更前保存原报告、输入含义、源revision、dirty paths、工具链和硬件。矩阵seed=1,2,3,4,5,7,11,19,23,31；默认5股、20000散户、5机构、2游资、30交易日；另保留压缩300tick/日成本场景，二者不混报。禁止将旧格式支持永久嵌入产品。
  - 依赖/并行：W1；无；阻塞2、3；首个执行。
  - References：`docs/diagnostics.md`、`docs/price-volume-simulation-gap-checklist.md:258–271`、`packages/engine/examples/price_volume_baseline.rs`、`packages/engine/tests/diagnostics.rs`、`package.json`。
  - Acceptance：before报告对账成功，10个独立seed和每股极端结果保留，原始JSON与运行清单存E/before/；fixture产生新会话，不恢复/改写用户存档。
  - QA happy：Shell `node scripts/simulation/baseline-run.mjs before --output .omo/evidence/company-information-npc-intentions/before`，内部调用真实release示例，E/task-1-happy.txt。
  - QA failure：`node --test scripts/simulation/baseline-run.test.mjs`覆盖重复seed、子进程非零退出、无报告、报告非对应配置时拒绝，E/task-1-failure.txt；人工式agent核对报告来源，不以脚本打印success替代。
  - Commit：Y；`test(engine): 固定公司模型变更前量价基线`。

- [x] 2. 固定官方依据、会计政策和获准范围
  - 实施：更新ADR0016、相关ADR0013/0015、open-questions；新增`docs/company-accounting.md`、`docs/simulation-calendar.md`、`docs/company-actions-design.md`及机器可读`packages/engine/tests/fixtures/company-model/policy-sources.json`。登记每业务对应准则条款、生效日期、来源摘要、取证日期、适用公司/报表、游戏假设和不支持合同；股东结算文档仅设计借贷/权益/登记/税费的未来接口边界，不建运行时代码。
  - 官方来源：已核对MOF `https://kjs.mof.gov.cn/zhengcefabu/202608/t20260805_3994927.htm`；须补MOF基本准则及收入14、借款费用17、所得税18、金融工具22/23/37、保险合同25、中期32、合并33、现金流31等适用文本，CSRC现行披露办法和沪深上市规则/休市通知。只用官方正文；搜索失败可手动浏览/获取官方附件，不以代理摘要充当原文。
  - 依赖/并行：W1；1；阻塞4、6、7及法定映射；可与3只读/抽取并行。
  - References：K1–K4、`docs/principles.md`、`docs/trading-rules.md`、`docs/decisions/0016-fundamental-factor-model.md`、`docs/open-questions.md:98–107`。
  - Acceptance：会计覆盖矩阵每行有来源或明确游戏假设；规则时间与模拟日期分开；用户已批准事项无重新采访；无法取得文本的领域任务显式BLOCKED，不能默认声称合法。会计范围和未来休市算法完全写明，源表不含虚构哈希/日期。
  - QA happy：agent使用Read/Webfetch读取原文及所写政策，逐条填写E/task-2-happy.txt，运行新增`cargo test -p engine --test policy_manifest`校验版本区间/来源结构，不测试自然语言措辞。
  - QA failure：同测试包含重叠适用版本、缺source、future-official但无正式通知、UnsupportedContract映射缺失拒绝；E/task-2-failure.txt。独立语义reviewer审查全文，禁止仅grep关键字。
  - Commit：Y；`docs(engine): 明确公司会计日历与不执行股东结算边界`。

- [x] 3. 按触及责任抽取现有会话与策略接缝
  - 实施：从`session.rs`抽到新增`session/execution.rs`、`session/attention.rs`、`session/views.rs`；从`strategy.rs`/`behavior.rs`抽到`strategy/`、`behavior/`子模块，先保留公共re-export。移动本次要变更的类型/函数及相关单测，保留其行为；不重排随机消耗/撮合顺序、不在本任务删除V。新文件超过250纯逻辑行按自然责任再分；不以include片段绕开责任拆分。
  - 依赖/并行：W1；1；阻塞5、21、26；独占既有核心文件。
  - References：`session.rs:420–465,2235–2500,2868–3070,4306–4391`、`strategy.rs`、`behavior.rs`、`docs/architecture.md`。
  - Acceptance：公开接口/序列化契约保持，原测试全绿；相同before小场景逐事件与权威存档完全相同。不得趁机修 unrelated smells。
  - QA happy：`cargo test -p engine`和新增`cargo test -p engine --test extraction_replay`，E/task-3-happy.txt。
  - QA failure：extraction_replay对照故意扰动事件顺序/seed的测试输入必须检出差异；既有非法委托/原子恢复失败测试仍执行，E/task-3-failure.txt。
  - Commit：Y；`refactor(engine): 分离会话执行与个体观察接缝`。

- [x] 4. 实现真实公历及冻结的交易日历
  - 实施：新增`calendar.rs`、`calendar/{date,policy,holidays}.rs`、`calendar/data/`；按K1实现日期/星期/闰年、沪深正式覆盖、模拟未知年份政策、离线农历/节气事实表及来源。纯日期算法不需要第三方运行时依赖；事实表不能通过未经核验的网上样例拼凑。
  - 依赖/并行：W1；2；阻塞5、7；可与6并行。
  - References：K1、任务2calendar来源、`session.rs TradingPhase/SessionSetup`、`docs/decisions/0011-market-time-observations-and-position-risk.md`。
  - Acceptance：default2030-01-01、不同起点、2000–2099运行边界及1998–1999初始化专用覆盖、2032闰日/跨世纪算法；2000开局前360个交易日均可定位；同政策同日历结果，save冻结政策不被新表覆盖。
  - QA happy：`cargo test -p engine --test calendar`执行`gregorian_and_exchange_days`、`frozen_calendar_survives_new_defaults`，E/task-4-happy.txt。
  - QA failure：同命令执行非法2月30日、越界日期、缺事实表年份、重复/冲突官方覆盖、错误digest、调休周末不作为交易日，E/task-4-failure.txt。
  - Commit：Y；`feat(engine): 增加公历及可冻结交易日历`。

- [x] 5. 将自然日经营时钟接入权威推进
  - 实施：新增`session/civil_clock.rs`，扩展日期字段和civil事件；按K1休市推进、交易日收盘18:00阶段与自然日边界。保留tick/market-minute原有交易时序；发布压缩可裁剪显示，但不得跳过业务事件。超日期范围、重复日结先验证并原子失败。
  - 依赖/并行：W1；3、4；阻塞14、15、28。
  - References：K1、`session.rs step/phase/end_of_day`、`session/candles.rs`、`observation.rs`、`docs/decisions/0010-unified-host-protocol-and-local-refresh.md`。
  - Acceptance：周末3个自然日各处理一次利息/到期回调，市场分钟仅在交易推进；closed起点不挪日期，休市无成交和attention RNG消费，下一交易时点T+1正确。
  - QA happy：`cargo test -p engine --test civil_clock`执行`closed_days_accrue_without_trading`及`restored_period_boundary_is_exactly_once`，E/task-5-happy.txt。
  - QA failure：重复结算、乱序日时钟、跳过未处理due事件、超过2099上界拒绝且不部分改变账户；同命令，E/task-5-failure.txt。
  - Commit：Y；`feat(engine): 分离公司自然日与市场交易时间`。

- [x] 6. 建立原子复式记账与科目子账底座
  - 实施：新增`accounting/{mod,amount,journal,ledger,period,error}.rs`。实现K2金额、借贷行、来源唯一性、批次原子性、按科目/期间增量索引、现金流类别/非现金标识；来源事实与派生report边界不可混用。
  - 依赖/并行：W1；2；阻塞7–11；可与4/5并行。
  - References：K2、任务2准则与科目映射、`money.rs`、`account.rs`、项目serde/thiserror惯例。
  - Acceptance：借贷总额相等、现金/权益滚动正确；合法负权益可表示；金额大于JS安全整数精确往返，逐笔分录错误不会留半条账。
  - QA happy：`cargo test -p engine --test accounting`包含K2/Verification金样及serde往返，E/task-6-happy.txt。
  - QA failure：同命令覆盖不平衡、重复event、i128溢出、负现金、已封期间入账、fraction尾差处理；assert完整状态不变，E/task-6-failure.txt。
  - Commit：Y；`feat(engine): 增加原子会计分录与总账`。

- [x] 7. 建立公司实体、经营合同与开局账套
  - 实施：新增`company/{mod,spec,opening,counterparty,contracts}.rs`，实现发行人/行业/会计类型、固定集团关系、公司现金与外部对手方账、经营预算/授信、显式平衡的初始账套与前史配置。本任务不提前承诺尚未实现的2年经营前史；历史业务生成由14、报表构建由13、历史发布集装配由15完成，26统一接入新游戏。默认5股票仅新增公司映射与虚构经营配置，不改变交易类别/股本、不依据初始股价反推资产。
  - 依赖/并行：W1；2、4、6；阻塞8–12及17。
  - References：K2–K3、`apps/web/src/config/defaults.ts:28–78`、`session.rs StockSpec/SessionSetup`、`session/candles.rs`。
  - Acceptance：公司数/发行股票映射/股本精确匹配；opening平衡，外部商业对手方不是证券NPC，无NPC新增现金；四种独立测试实体可以创建。
  - QA happy：`cargo test -p engine --test company_opening`执行`all_company_kinds_open_balanced`、`initialization_does_not_pay_investors`，E/task-7-happy.txt。
  - QA failure：未知发行人、重复公司ID、集团循环、初始股本不符、无授信仍新增债务、不平衡账套拒绝；同命令，E/task-7-failure.txt。
  - Commit：Y；`feat(engine): 增加公司与独立经营账套`。

### W2 — 行业会计、报表与经营事实

- [x] 8. 实现工商经营与营运资金会计
  - 实施：新增`company/industrial/`处理采购/生产/销售/回款/费用/资本开支；新增`accounting/{inventory,fixed_assets,receivables,tax}.rs`及按责任拆分。按K3移动加权成本、履约收入、直线折旧、坏账/减值、借款/利息、当期及递延税，所有现金和非现金事项分开。
  - 依赖/并行：W2；6、7；阻塞9–14；先完成共享税务/资产/应收会计及接口，9–11随后分目录并行。
  - References：K2–K3、任务2工商/收入/税务政策、`accounting/`新底座。
  - Acceptance：库存数量/成本、应收应付、资产账面/累计折旧和利润现金对账；赊销不加现金、补收款不重复收入；经营亏损不是引擎错误。
  - QA happy：`cargo test -p engine --test industrial_accounting`执行订单→生产→赊销→回款→结息完整金样，E/task-8-happy.txt。
  - QA failure：无库存交付、重复回款、非法税率/计量政策、超授信/无现金付款产生正确拒绝或逾期业务事件，不补钱；同命令，E/task-8-failure.txt。
  - Commit：Y；`feat(engine): 接通工商经营与营运资金会计`。

- [x] 9. 实现银行合同会计与报表映射
  - 实施：新增`company/bank/`、`accounting/reports/bank.rs`；按K3存贷款、实际利率、ECL三阶段、核销回收及客户流动性约束，单独科目映射。PD/LGD与贴现参数显式配置，不从股票跌幅直接认定信用损失。
  - 依赖/并行：W2；6、7、8；阻塞12–14；与10、11分目录并行。
  - References：K3、任务2金融工具及银行列报官方映射、`accounting/journal.rs`。
  - Acceptance：存入1000贷客户存款而非收入，贷出600转贷款资产而非费用；利息应计、存款付息、阶段转移/减值及现金对账；默认股票不增加银行股。
  - QA happy：`cargo test -p engine --test bank_accounting`执行存贷→利息→减值→回收金样，E/task-9-happy.txt。
  - QA failure：超可支付现金提款为支付失败而不是负现金，非法PD/LGD、重复核销、结构化未支持合同明确拒绝；同命令，E/task-9-failure.txt。
  - Commit：Y；`feat(engine): 支持银行经营会计及报表分类`。

- [x] 10. 实现保险合同服务与负债计量
  - 实施：新增`company/insurance/`、`accounting/reports/insurance.rs`；K3一般计量模型、合同组、预期现金流/折现/风险调整/CSM、服务单元释放、赔付、亏损组和重估。保险股东分红及投连合同不执行，不把普通保费流变成股东支付。
  - 依赖/并行：W2；6、7、8；阻塞12–14；与9、11并行。
  - References：K3、任务2保险合同准则确认/计量/列报原文、`accounting/journal.rs`。
  - Acceptance：保费收到日现金增加但不全部计保险服务收入，服务后正确释放；赔案发生与支付分开，合同负债变动能与利润/现金/CSM调节表对账。
  - QA happy：`cargo test -p engine --test insurance_accounting`执行盈利组、亏损组、估计改变和赔案金样，E/task-10-happy.txt。
  - QA failure：负服务单元、非法贴现率、支付超过现金、将分红/投连/再保险合同送入未支持路径明确拒绝；同命令，E/task-10-failure.txt。
  - Commit：Y；`feat(engine): 支持保险服务与合同负债报表`。

- [x] 11. 实现地产开发、预售与交付会计
  - 实施：新增`company/real_estate/`、`accounting/reports/real_estate.rs`；项目成本归集、开发库存、符合条件资本化、预售合同负债、交付控制权转移、收入成本及减值。资本化暂停/结束依据任务2固定政策，不永远把利息藏进资产。
  - 依赖/并行：W2；6、7、8；阻塞12–14；与9、10并行。
  - References：K3、任务2收入/存货/借款费用规则、`accounting/`。
  - Acceptance：预售收款不确认交付收入，实际交付冲合同负债并结转成本；项目余额、应收、现金、利润勾稽；不要求默认地产股。
  - QA happy：`cargo test -p engine --test real_estate_accounting`执行购地→开发→预售→暂停→交付→收尾金样，E/task-11-happy.txt。
  - QA failure：未达到交付条件确认收入、重复交付、无限资本化、超项目数量及无现金支付正确拒绝；同命令，E/task-11-failure.txt。
  - Commit：Y；`feat(engine): 支持地产预售交付与项目会计`。

- [x] 12. 增加固定集团合并与少数股东权益
  - 实施：新增`accounting/consolidation/`；固定控制关系、单体汇总、往来/内部销售/未实现利润抵销、少数股东损益权益。抵销凭证属于合并工作底稿，不重复记到子公司现金；不扩展并购或股权交易。
  - 依赖/并行：W2；7–11；阻塞13；等行业接口稳定后单owner。
  - References：K3、任务2合并准则、`company/spec.rs`、`accounting/ledger.rs`。
  - Acceptance：母公司80%子公司金样准确区分少数股东；内部赊销抵销后集团现金不变；单体/合并ScopeId明确；无子公司返回NotApplicable。
  - QA happy：`cargo test -p engine --test consolidation`执行混合行业固定集团与内部库存未售利润金样，E/task-12-happy.txt。
  - QA failure：循环/重复合并、控制与持股不一致、缺相同会计期间、对手方余额不符必须显式错误，不用差额plug平账；同命令，E/task-12-failure.txt。
  - Commit：Y；`feat(engine): 增加固定集团合并与抵销`。

- [x] 13. 生成完整报表、附注及不可变期间版本
  - 实施：新增`accounting/reports/{mod,industrial,balance_sheet,income,cash_flow,equity,notes}.rs`和`accounting/closing.rs`，接入行业映射及合并。期间试算→报表→现金/权益/附注勾稽→封账；更正以新版本和调整分录处理，保留原版本。
  - 依赖/并行：W2；8–12；阻塞15；可与14并行。
  - References：K3、任务2列报/现金流/中期政策、`accounting/period.rs`、`accounting/reports/{bank,insurance,real_estate}.rs`。
  - Acceptance：四类均输出四张基本表+附注；期初期末/上期同比/当季和累计明确，缺历史不填零；报表由分录推导可重建，快照不得独立随机化。
  - QA happy：`cargo test -p engine --test industry_reports`对四种公司/合并Scope运行五产物及间接CF金样；另覆盖每日定稿→同年后续月入账→年结→更正年报，原公开版本不变，E/task-13-happy.txt。
  - QA failure：科目无归属、重复分类、附注明细合计不符、试算不平不允许公布；合法Unavailable(reason)比较项可公布，但缺原因或捏造0拒绝；同命令，E/task-13-failure.txt。
  - Commit：Y；`feat(engine): 生成行业财务报表与版本化结账`。

- [x] 14. 接通经营演化、合同到期及市场行业事件
  - 实施：新增`company/{operations,events,scheduler,rng}.rs`及`session/company_operations.rs`。按K4推进自然日需求/供给/履约/收付/信用，不直接随机改财务余额；跨行业事件只作用于适用经济字段。公司/行业/市场RNG分流，事件队列稳定排序。同一处理器提供初始化专用的2年及当年截至开局前日的历史业务生成，不创建历史证券成交，也不在本任务抢先组装公开报告。
  - 依赖/并行：W2；5、8–11；阻塞15、26；与12/13分模块。
  - References：K2/K4、`session/civil_clock.rs`、四行业处理器、`session.rs SplitMix64`。
  - Acceptance：同seed同事件/分录序列；公司违约生成业务风险，未执行任何股东分配；公司经营不需要股票有成交；行业标签不读证券类别。
  - QA happy：`cargo test -p engine --test company_operations`覆盖共同需求冲击对不同公司不同经营响应，E/task-14-happy.txt。
  - QA failure：无授信资金断裂、停工/交付失败、重复due事件、将股东动作送scheduler明确不支持；同命令，E/task-14-failure.txt。
  - Commit：Y；`feat(engine): 接通自然日经营与共同经济事件`。

### W3 — 披露、个体认识与持续计划

- [x] 15. 实现定期报告、临时公告和不可变公开信息库
  - 实施：新增`information/{mod,publication,schedule,public_view}.rs`、`session/disclosures.rs`；按K4生成定期/临时披露，分离发生/报告期/公布时点。公开更正是新版本关联旧ID，历史不可覆写；只有业务范围定稿且勾稽通过的报告可公开。调用14历史业务和13报表构建，以真实历史排期组装开局已公开版本，开局未来才公布的报告不得提前纳入。
  - 依赖/并行：W3；5、13、14；阻塞16、18、29；与17/19/20/21分模块。
  - References：K4、任务2披露规则、`accounting/closing.rs`、`session/company_operations.rs`。
  - Acceptance：年报先于Q1、季/半年/年报告窗口均合规；非交易日18:00也能公开且无撮合；同seed排期/ID一致，report版本保存后不变；2000-01-01最早开局的1998起财务前史和360个交易日预置K均合法且可恢复。
  - QA happy：`cargo test -p engine --test publications`执行`weekend_report_publishes_without_trade`、`correction_preserves_previous_version`，E/task-15-happy.txt。
  - QA failure：提前读取、发布未结账/不平报表、时间逆序、非法报告窗口/重复ID拒绝，不以补默认日期绕过；同命令，E/task-15-failure.txt。
  - Commit：Y；`feat(engine): 增加财务披露与公开版本库`。

- [x] 16. 将公共曝光与NPC本人获取信息分离
  - 实施：新增`information/{acquisition,npc_view}.rs`，按个人关注/观察生成报告ID与获知时点；新 `NpcObservationContext`只包含本人已获取的公开信息引用和可见行情，不让策略访问CompanyState总账。按报告ID索引重用不可变内容，个人状态不克隆所有报表。
  - 依赖/并行：W3；15；阻塞18、25、26；可与17/19/20/21并行。
  - References：K4–K5、`session/attention.rs`、`observation.rs`、`strategy.rs MarketView/SelfView`。
  - Acceptance：改变未披露事实、或已披露但本人未读的内容，不改变该NPC观察前判断；一次获知只记一次，dev查看不算NPC阅读。
  - QA happy：`cargo test -p engine --test information_acquisition`运行逐人延迟获知、重复阅读、历史版本金样，E/task-16-happy.txt。
  - QA failure：未来publication、未知report ID、observed_at早于published_at、他人信息集注入拒绝；同命令，E/task-16-failure.txt。
  - Commit：Y；`feat(engine): 隔离公开曝光与个人已知信息`。

- [x] 17. 将账户身份、主导风格和分析权重解耦
  - 实施：新增`strategy/{analysis_profile,factory_profiles}.rs`，扩展工厂输出K5配置。明确每类方法选择、个体抽样和整数归一余数规则（最大余数法、字段顺序破同分）；保留原账户ordinal映射/身份和主导风格。参数可序列化，新局确定性构造且无共同V能力字段。
  - 依赖/并行：W3；2、7；阻塞18–20、26；与15/16/21并行。
  - References：K5、`strategy.rs StrategyProfile/StrategyFactory/StrategyParams`、`tests/strategy.rs`、清单A01/A08。
  - Acceptance：同机构身份可无基本面、散户可有基本面，零权重真正不运行该分析；主导风格不每次抽样；weight总和10000，实体验证不以名字推断能力。
  - QA happy：`cargo test -p engine --test analysis_profiles`运行工厂重放/跨身份组合/零基本面路径，E/task-17-happy.txt。
  - QA failure：负权重、全0、非法方法/行业映射、恢复profile与config冲突显式拒绝；同命令，E/task-17-failure.txt。
  - Commit：Y；`feat(engine): 分离NPC身份与混合分析能力`。

- [x] 18. 实现个人基本面预测、可选估值与信息驱动更新
  - 实施：新增`strategy/fundamental/{forecast,valuation,update}.rs`、`beliefs.rs`；按K5实现现金流、盈利倍数、权益/ROE路径，个人增长/资本成本/盈利质量假设及区间/期限/信心。分析方法适用性由report kind与当前可用数据判定，不强行共用一个“fair value”。
  - 依赖/并行：W3；13、16、17；阻塞22、23、26；与19–21并行。
  - References：K5、`strategy.rs:423–480,617–631,TargetPolicy`、新报表/NpcObservationContext。
  - Acceptance：利润增10%、先前预期增30%与预期下滑的受控参与者可作相反修订；无新信息/到期/经历触发时保持估值；亏损和負权益不产生正值fallback；个人区间不作为价格设定输入。
  - QA happy：`cargo test -p engine --test fundamental_beliefs`验证三种方法独立金样、same-news-different-priors、no-trigger-stable；明确整体归母权益估值100万元/总股数10万股=每股10元，仅改变流通股数估值不变，并验证少数股东剔除，E/task-18-happy.txt。
  - QA failure：g>=r、零/负倍数分母、不支持report/缺历史、未来信息、结果溢出显式Unavailable/typed error且无偷偷改股价；同命令，E/task-18-failure.txt。
  - Commit：Y；`feat(engine): 基于个人已知财务形成异质预期`。

- [x] 19. 扩展个人价格记忆与技术分析观测
  - 实施：新增`strategy/technical.rs`、`observation/technical.rs`、`experience/price_memory.rs`（必要时将experience.rs改为模块入口）；保留原30分钟/5日/区间/量价信号，按K5新增SMA/RSI/ATR。个人见过的价与公开历史计算信号区分，不虚构观察经历。
  - 依赖/并行：W3；17；阻塞22、23、25、26；与18/20/21分模块。
  - References：K5、`observation.rs`、`behavior.rs`、`experience.rs`、ADR0011/0012、`tests/behavior.rs`。
  - Acceptance：不读取基本面也能决策；短期下跌/长期上行可同时表达；窗长按完整市场时间，不依赖宿主tick密度；原行情信号断言保留。
  - QA happy：`cargo test -p engine --test technical_memory`覆盖固定价格序列独立金样、本人前高/前低及主动读取历史，E/task-19-happy.txt。
  - QA failure：短历史、无成交、缺日/分钟、未来高低点、RSI零涨跌分母定义明确（全平为中性50且带有效样本）或拒绝，不能补虚构走势；同命令，E/task-19-failure.txt。
  - Commit：Y；`feat(engine): 扩展个人参照与具名技术信号`。

- [x] 20. 将真实经历接入信心、忍耐与风险压力
  - 实施：扩展现有经历模块，按K5保留订单去重、失败买入、成本/峰值/冷静期；添加经历日期、长期被套、失败影响衰减和恢复规则，将字段真正接入目标/紧迫度输入。复用状态，不另建一份账户损益。
  - 依赖/并行：W3；17；阻塞22、23、25、26；与18/19/21并行，experience主入口单owner协调。
  - References：K5、`experience.rs:118–240`、`behavior.rs decide_retail_position_with_experience`、ADR0013、清单B03。
  - Acceptance：同损益不同经历在受控风格下选择不同；补仓不清除账户损失；清仓再入不抹去冷静期历史；未成交计划不能记失败交易；保留非恐慌者不必止损的差异。
  - QA happy：`cargo test -p engine --test experience`和新增`cargo test -p engine --test experience_feedback`，E/task-20-happy.txt。
  - QA failure：部分成交重复计次、未来经历、丢失持仓生命周期、非法峰值/衰减时间回拨拒绝，E/task-20-failure.txt。
  - Commit：Y；`feat(engine): 用真实经历调整个体耐心与风险行为`。

- [x] 21. 建立可跨日的个人交易计划状态机
  - 实施：新增`plans/{mod,state,revision,validation}.rs`，实现K6 PlanId、目标、原因、触发器、状态、版本与真实filled进度；与现有ParentOrderPlan执行子状态建立引用，不创建第二套订单/冻结。纯状态机先落地，真实路由在24/26接入。
  - 依赖/并行：W3；3；阻塞22–24；可与15–20并行。
  - References：K6、ADR0015/0016、`session.rs ParentOrderPlan`、`behavior.rs PositionDecision`。
  - Acceptance：连续观察无变化不重置方向/filled；暂停、反向修订、到期终止各有明确原因；日终仅子单生命周期结束，不将个人计划标Completed。
  - QA happy：`cargo test -p engine --test plans`执行continue/revise/pause/resume/terminate/真实完成及跨日状态金样，E/task-21-happy.txt。
  - QA failure：filled超过目标、修订目标小于已成交却未解释终止、过期复活、accepted当fill、未知PlanId关联拒绝；同命令，E/task-21-failure.txt。
  - Commit：Y；`feat(engine): 增加可追溯的持续个人交易计划`。

### W4 — 预算、紧迫度、执行与权威替换

- [x] 22. 增加账户内跨股票比较与软预算分配
  - 实施：新增`plans/{allocation,candidates}.rs`；按K6本人持仓/关注/本轮发现范围比较持有/换股/现金，按确定性优先级分配软预算。使用权威可用cash/reservations/fees/risk，不预支未成交卖款，不重复将已冻结预算扣两次。
  - 依赖/并行：W4；18–21；阻塞24、26；与23、25并行。
  - References：K6、`account.rs`、`session.rs reservation helpers`、`strategy.rs risk_capped_buy_qty`、`behavior.rs target_position`。
  - Acceptance：同账户两股各申请80%时总分配不超预算；费用最低佣金入预算，现金保留可使认为便宜仍不买；顺序/线程改变不改分配结果。
  - QA happy：`cargo test -p engine --test plan_allocation`覆盖双股竞争/风险退出优先/保留现金/部分成交后重分配，E/task-22-happy.txt。
  - QA failure：pending卖出不能支付买入、冻结不一致、费用超现金、不可卖库存与未关注候选被正确限制；同命令，E/task-22-failure.txt。
  - Commit：Y；`feat(engine): 协调NPC跨股票计划与真实预算`。

- [x] 23. 分离观点与执行紧迫度并实现报价决策
  - 实施：新增`plans/{urgency,quote_policy}.rs`；按K6基本判断保持但因下跌加速/风险/经历可暂停或撤买单。输出Wait/Keep/Cancel/Replace/Submit动作及理由，价格/数量仍由合法路由预校验，不直接改盘口。
  - 依赖/并行：W4；18–21；阻塞24、26；与22、25并行。
  - References：K6、`session.rs NpcOrderLifecycle`、`strategy.rs Intent`、`orderbook.rs`、清单A05/A06/A14。
  - Acceptance：固定估值仍看好但暂撤买单；紧迫卖方更积极报价但现金/持仓规则不变；有双边盘口才取对手报价，无盘口明确等待/保护限价策略，不创造流动性。
  - QA happy：`cargo test -p engine --test urgency`覆盖Patient/Normal/Urgent、cheap-but-withdraw、流动性下降，E/task-23-happy.txt。
  - QA failure：价格越涨跌停/非法tick、不可撤阶段的Cancel被约束，空簿不假设成交；同命令，E/task-23-failure.txt。
  - Commit：Y；`feat(engine): 分开个体判断与交易紧迫度`。

- [x] 24. 复用母单和工作单实现统一计划执行
  - 实施：修改抽取后的`session/execution.rs`并新增`session/plan_execution.rs`；把21–23输出接到真实submit/cancel/fill回写，普通挂单寿命与母单子单唯一关联。旧报价被认领仅限完全同价同向合法子单，变价必须真实撤报。
  - 依赖/并行：W4；21–23；阻塞26、27；与25并行。
  - References：K6、`session.rs`原`materialize_parent_order_intents/reconcile_npc_working_orders/record_parent_order_*`、`session/persistence.rs`、ADR0015。
  - Acceptance：每账户每股至多一个在途子单；部分成交/集合竞价转连续/收盘竞价/日终释放正确；真实成交推进plan，次日下次观察才重报；Queue priority不偷保留。
  - QA happy：`cargo test -p engine --test plan_execution`运行真实对手方400股计划部分成交及跨日恢复场景，E/task-24-happy.txt。
  - QA failure：09:20后及收盘不可撤时不发矛盾单、零股剩余不可超买、撤失败不得提前释放预算，E/task-24-failure.txt。
  - Commit：Y；`feat(engine): 以真实订单生命周期执行持续计划`。

- [x] 25. 扩展个人关注发现、保留与淡出
  - 实施：修改`session/attention.rs`和`experience/watchlist.rs`；继承原候选概率/个体RNG，用异常30分钟涨跌/相对量能/公告曝光提高候选发现权重。持仓及活跃计划股票不可淡忘；未持仓/无计划上限8，按最后实际关注时间及StockCode稳定修剪；新曝光不等于已读。
  - 依赖/并行：W4；16、19、20、21；阻塞26、27；与22–24并行。
  - References：K4–K6、`experience.rs prune_watchlist/observe_stock`、`behavior.rs select_observed_stock`、`session.rs npc_attention`、清单A13。
  - Acceptance：保留60%持仓优先基础及全市场发现机会，未关注异常股可被发现；一次公共公告不让全体同步交易；计划终止后可淡出，未披露事实不改变发现权重。
  - QA happy：`cargo test -p engine --test attention_discovery`验证固定seed加权候选、计划保留、无计划LRU和恢复，E/task-25-happy.txt。
  - QA failure：只变私有经营事实不改候选/订单、cap超限不能删持仓/活跃计划、非法观察时间拒绝；同命令，E/task-25-failure.txt。
  - Commit：Y；`feat(engine): 以公开曝光扩展个体关注生命周期`。

- [x] 26. 接通完整决策链并彻底删除共同V
  - 实施：整合`session.rs/strategy.rs/market.rs/compute.rs`及其抽取模块：自然日经营/披露→accepted注意力→本人信息→K5a混合分析/经历→持续计划→预算/紧迫度→执行；将7/13/14/15完整初始化接入new session。删除`fundamental_value/v_initial/VParams/fundamental_value_means/evolve_v/TrackV/needs_fundamental_value`及with-V/no-V分支、Rust配置和GPU实验关联接口；ComputeBackend保留纯批量输入/稳定顺序输出，不按账户身份偷授予信息。Rust宿主fixture同批迁移使workspace可编译；TS默认setup在29生成新类型后迁移。
  - 依赖/并行：W4；3、14、16–20、24、25；阻塞27–29；独占核心整合。
  - References：K2/K4–K6、`market.rs:68,206–229,297–356`、`strategy.rs:423–480,617–631,686,780`、`compute.rs`、`packages/engine-gpu/`、`apps/web/src/config/defaults.ts`。
  - Acceptance：所有可执行/序列化/配置的共同V路径消失；历史ADR可保留已实施历史并标被替代，不能改史；CPU各线程数、diagnostics开关相同事件/状态。新的基本面/技术路径都能在真实GameSession执行，比较市场报价只消费K5a按总股数换算后的个人每股估值区间，拒绝整体权益量纲误接。
  - QA happy：`cargo test -p engine --test company_decision_session`及`cargo test --workspace`覆盖全部策略风格和ComputeBackend，E/task-26-happy.txt。
  - QA failure：no-counterparty→零成交，unpublished mutation→同个人决策，非法analysis input→明确错误；结构搜索仅补充V删除清单，不替代行为测试，E/task-26-failure.txt。
  - Commit：Y；`feat(engine): 以公司信息和个体判断替换共同V`。

- [ ] 27. 扩展新格式完整存档并移除旧格式专用路径
  - 实施：修改Rust `session/persistence.rs`、save/restore和Rust fixtures；K7新增必填状态，精确检查公司/账户集合、分录/余额、日历身份、报告引用/时序、计划/子单/冻结一致性及RNG。删除Rust旧格式转换/兼容和旧V专用测试；建立E/compatibility-removal.md逐项映射，保留同一业务的新模型测试。TS validator/旧schema_version特判/TS fixture删除移到29，WASM normalization到30，不提前要求旧TS类型适配未生成字段。
  - 依赖/并行：W4；26；阻塞28–32；与其他shared save修改串行。
  - References：K7、`session.rs SaveSlot/save/restore`、`session/persistence.rs`、`apps/web/src/save/save-schema.ts:148–202`、`save-repository.test.ts`。
  - Acceptance：新格式JSON往返和恢复后权威状态精确相同；旧形状只是当前schema不合法，走通用错误，无专用legacy分支/迁移器；不删当前字段缺失/精度/原子失败断言。
  - QA happy：`cargo test -p engine --test save_contract`及Rust当前恢复回归，E/task-27-happy.txt；Web完整测试待29/30完成后执行。
  - QA failure：伪报告引用、未来观察、丢失个人state、错误日历/账本digest、超大小、未知字段/缺字段全部拒绝，原运行session和输入文件字节不变；同命令，E/task-27-failure.txt。
  - Commit：Y；`feat(engine): 固化公司与个体状态的新存档契约`。

- [ ] 28. 锁定端到端会计—信息—计划—撮合场景
  - 实施：新增`tests/company_scenarios.rs`及`tests/fixtures/company-model/`受控公司/参与者配置；完整GameSession跨年/报告/休市/交易日/部分成交/恢复，对照诊断开关和同seed不中断实例。会计事件可fixture注入，成交必须真实订单簿。
  - 依赖/并行：W4；5、26、27；阻塞36、37；与29并行。
  - References：K1–K7、Verification金样、清单ADR0016验收65–76、`tests/session.rs`。
  - Acceptance：无前视、同消息不同先验、cheap-but-withdraw、同损益不同经历、跨股卖未成交不买、无承接零成交、会计/Trade/日K/账户/plan全面对账；不要求自然随机每次产生分歧。
  - QA happy：`cargo test -p engine --test company_scenarios`，E/task-28-happy.txt和场景结构化输出。
  - QA failure：用显式非法输入构造非平账/未来report/超预算/T+1卖/无对手方，断言错误路径或合法零成交而非模拟修补；同命令，E/task-28-failure.txt。
  - Commit：Y；`test(engine): 锁定公司披露到真实撮合的场景闭环`。

### W5 — 三宿主、公共信息与开发诊断

- [ ] 29. 固定公共报告查询与事件契约并生成TS类型
  - 实施：新增engine `company/query.rs`公共查询DTO；扩展`Event/Snapshot/HostCapabilities`及`apps/web/src/types/engine.ts`、EngineHost定义。K7页码/游标、民用日期、report revision、civil/disclosure事件、报告按ID查询；Rust→TS只用生成脚本，不手写生成目录。生成后同步`config/defaults.ts`、TS save validator及fixtures，删除旧schema_version专用特判并补任务27删除清单；真实WASM相关全套Web门禁待30新绑定就绪后运行。
  - 依赖/并行：W5；15、26、27；阻塞30–33；共享生成目录唯一owner。
  - References：K7、`apps/web/src/host/`接口、`apps/web/src/types/generated/`、`scripts/check-generated-types.mjs`、ADR0010。
  - Acceptance：i128/u64等无损字符串，public DTO不含未披露总账；公司非上市也可在测试查询但默认行情列表不增加股票；分页稳定、旧report可查询。
  - QA happy：`pnpm types:generate`、`pnpm types:check`、`cargo test -p engine --test company_query_contract`，E/task-29-happy.txt。
  - QA failure：未知公司/报告、未公开ID、page_size0或>100、非法游标、超精度数值拒绝；契约不允许把无比较期当0，E/task-29-failure.txt。
  - Commit：Y；`feat(engine): 定义公共公司报告与日历协议`。

- [ ] 30. 接通WASM Worker公司信息与新存档
  - 实施：修改`apps/web-wasm/src/lib.rs`、`apps/web/src/host/{wasm-host,wasm-worker}.ts`及serde normalizer；WASM函数只桥接引擎公共查询/日期/恢复，新增BTreeMap和十进制字段正确跨JSON/JS。重新生成wasm-pkg，不人工修改绑定。
  - 依赖/并行：W5；27、29；阻塞34、35、37、40；与31/32/33并行。
  - References：K7、`apps/web/src/host/serde-normalize*`、`apps/web-wasm/src/lib.rs`、`scripts/wasm-build.bat`/`.sh`。
  - Acceptance：真实WASM实例创建2030休市起点，查询已公开报表、运行并恢复；Map键/金额/报告ID无丢失，事件seq完整。
  - QA happy：执行平台WASM构建脚本，`cargo test -p web-wasm`、`pnpm --filter web test`；E/task-30-happy.txt记录实际命令。
  - QA failure：真实worker收到损坏save/未知query、超时后迟到旧session响应，明确错误且当前游戏不被旧响应覆盖；新增host测试执行，E/task-30-failure.txt。
  - Commit：Y；`feat(web): 接通WASM公司报告与自然日期`。

- [ ] 31. 接通Server公共查询、休市事件及资源门禁
  - 实施：修改`apps/server/src/{routes,actor,publisher}.rs`；新增只读`GET /api/companies/{id}/reports?cursor=...&limit=...`及`GET /api/companies/{id}/reports/{report_id}`，沿用会话鉴权/actor串行。civil/publication事件触发runtime/public revision，WS断线重连baseline包含最新公开索引。扩展新状态导入资源预检，不能先无限反序列化后才拒绝。
  - 依赖/并行：W5；27、29；阻塞35、37、40；与30/32/33并行。
  - References：K7、`apps/server/src/routes.rs:47–120`、`apps/server/tests/{api_contract,publisher,ws}.rs`、ADR0010。
  - Acceptance：无交易休市日公告从真实HTTP/WS可见，响应未泄露未公布账本；导入失败不替换actor状态；最大请求限制包括实际body bytes/集合大小。
  - QA happy：`cargo test -p server`含新增`company_api`集成测试，E/task-31-happy.txt。
  - QA failure：无鉴权、他局报告ID、未公开ID、超body/页大小、损坏档HTTP明确4xx并保持会话；断连后序列重新同步，E/task-31-failure.txt。
  - Commit：Y；`feat(server): 发布公司信息并保护新状态边界`。

- [ ] 32. 接通Tauri公司查询与日期事件
  - 实施：修改`apps/desktop/src-tauri/src/{lib,actor}.rs`与`apps/web/src/host/tauri-host.ts`；增加统一公共查询对应IPC command、日期/披露事件、恢复原子替换及查询session generation检查。不从desktop直接import web/server业务。
  - 依赖/并行：W5；27、29；阻塞35、37、40；与30/31/33并行。
  - References：K7、`apps/desktop/src-tauri/src/lib.rs save_session/restore_session`、actor、`tauri-event-coordinator.test.ts`。
  - Acceptance：actor提供与WASM/Server相同的已公开报告，休市更新不被交易事件过滤；保存JSON重新读入精确一致。
  - QA happy：`cargo test --workspace`及`pnpm --filter web test`含新增Tauri公司查询契约，E/task-32-happy.txt。
  - QA failure：错误IPC参数、未知report、旧generation响应和失败恢复均不污染当前session；同命令，E/task-32-failure.txt。
  - Commit：Y；`feat(desktop): 接通公司报告与自然日更新`。

- [ ] 33. 建立公共公司状态和一致的前端更新规则
  - 实施：新增`apps/web/src/store/company-slice.ts`、`apps/web/src/host/company-query-coordinator.ts`，修改runtime snapshot policy/event buffer/三个adapter解码。缓存按session+company+report版本，baseline原子替换；delta失序沿用重同步，不用UI补猜财务。
  - 依赖/并行：W5；29；阻塞34、35；与三个宿主实现并行。
  - References：K7、`apps/web/src/host/runtime-snapshot-policy.ts`、`event-buffer*`、`apps/web/src/store/`、ADR0010。
  - Acceptance：CivilDateAdvanced/CompanyDisclosurePublished无成交也更新公共状态；新局不继承旧缓存；未发布、无比较期、加载/错误各有明确状态；金额不转换为不安全Number。
  - QA happy：`pnpm --filter web test`新增company-slice/query-coordinator测试，E/task-33-happy.txt。
  - QA failure：乱序seq、旧query返回、新旧公司同report序号、缺字段/大整数处理明确拒绝或重同步，E/task-33-failure.txt。
  - Commit：Y；`feat(web): 原子同步公共公司与报告版本状态`。

- [ ] 34. 增加公开财务界面与新游戏日期选择
  - 实施：新增`apps/web/src/components/company/`中的`CompanyPanel/FinancialStatementTable/DisclosureList/ReportNotes`、`apps/web/src/components/StartDateInput.tsx`及格式化/测试；接入App桌面面板和现有移动详情信息菜单。更新DESIGN.md新增状态/primitive后实现，不做视觉改版；共用公司内容，不按host写分支。
  - 展示：当前自然日期/是否休市/模拟日历标识、公司业务/行业、财报期间/公告时刻/版本/单体或合并、四张表及附注/上期对比。移动表格内部横向滚动、冻结科目列，不让整页溢出；金额显示元/万元/亿元并可查看精确值，缺数据给原因不显示0；会计负数不简单等同股价涨跌颜色。
  - 依赖/并行：W5；30、33；阻塞37、40、41；可与35分目录。
  - References：K1/K7、`DESIGN.md`全文、`apps/web/src/App.tsx`、`apps/web/src/mobile/`、`apps/web/e2e/trading-workflows.spec.ts`、现有Blueprint/AG Grid及金额格式化工具。
  - Acceptance：四行业测试公司均显示正确对应报表；默认股票不增加；2030默认和自选有效/无效日期可验证；公告在休市中出现不用刷新；键盘可操作，loading/error/empty/更正版本可区分。
  - QA happy：新增`apps/web/e2e/company-information.spec.ts`；`pnpm --filter web exec playwright test e2e/company-information.spec.ts`，375/768/1280截图、读表/切期间/切公司/日期创建，E/task-34-happy/；调用frontend与visual-qa要求独立视觉通过。
  - QA failure：无已公开报告、缺同比、超长中文/大额/负值、查询失败、非法日期、恢复后旧选择不存在；相同E2E明确断言，无浏览器人工代跑，E/task-34-failure/。
  - Commit：Y；`feat(web): 展示公开财务报表并支持模拟起始日期`。

- [ ] 35. 增加dev专用个体诊断并隔离产品release
  - 实施：新增`diagnostics/decision_trace.rs`及host debug query adapter、`apps/web/src/components/dev/NpcDecisionInspector.tsx`；K7选定NPC最近128条事件，来源report/预期方法/plan变化/预算限制/真实订单ID可追溯。Rust debug_assertions与显式离线`simulation-diagnostics`功能在编译层隔离，Web DEV动态import；不通过全量公开snapshot发私有数据。
  - 依赖/并行：W5；30–33；阻塞36、37、40；与34并行。
  - References：K7、`session.rs RetailDecisionTrace/RetailOrderDiagnosticEvent`、`diagnostics.rs`、`MarketGrid.tsx import.meta.env.DEV`、三个host接口。
  - Acceptance：只开debug不改变状态/RNG；release无debug command/route/worker消息分支及collector分配；权威plan原因、save状态与正常错误仍存在。离线优化诊断产物与产品分发目录完全分开。
  - Feature边界：为每个依赖诊断的integration test/example显式声明Cargo `required-features=["simulation-diagnostics"]`；其测试模块不能在默认workspace构建中无条件import已裁剪类型。prod-absence测试不设该required-feature；default全套和feature全套分别报告实际target/测试数，不能把未编译诊断测试误称覆盖。
  - QA happy：`cargo test -p engine --features simulation-diagnostics --test diagnostic_parity`、`pnpm --filter web test`和dev inspector E2E，E/task-35-happy.txt。
  - QA failure：`cargo test -p engine --release --test diagnostic_absence`确保特性禁用时能力缺席，host收到debug请求明确不支持、不泄露私有状态；完整构建证明留到40，E/task-35-failure.txt。
  - Commit：Y；`feat(web): 增加仅开发可用的NPC因果诊断`。

### W6 — 统计、跨宿主、规模与最终交付

- [ ] 36. 扩展离线因果诊断与量价指标
  - 实施：扩展`packages/engine/src/diagnostics.rs`按责任拆到`diagnostics/`，改造`examples/price_volume_baseline.rs`及Cargo required-features；新增公司/信息/plan/执行聚合，分析前向引用与预算约束，订单寿命、主动撤单/改价/日终取消分类、参与率、成交率、方向持续性、点差/深度和恢复时间分布。
  - 指标口径：信息延迟=获知civil instant减publication；订单寿命分别记录市场分钟和自然秒；成交率=filled/submitted，不用OrderAccepted数代替提交数；方向相关按实际同股票主动成交序列；恢复时间只在真实深度损失后观测回到损失前50%深度且双边报价有效的首个市场分钟，报告未恢复删失样本；冲击取执行前中价到成交后首个有效中价的有符号bp并注明缺样本，不能暗称因果估计。
  - 依赖/并行：W6；28、35；阻塞38、39、42；可与37并行。
  - References：K4–K7、`docs/diagnostics.md`、清单C01–C07/§6、`tests/diagnostics.rs`、现有retail_execution/participant_execution。
  - Acceptance：新增指标来自实际事件、可追到seed/公司/订单/计划；无样本null+原因；Trade/日K量额、参与双边量=2×市场量、提交=成交+撤销+未结+aborted继续硬对账；诊断RNG无副作用。
  - 构建：同步现有`tests/diagnostics.rs`及全部诊断依赖target的required-features，示例脚本调用增加`--features simulation-diagnostics`。非诊断session/scale默认测试仍必须可编译并运行；任务1before命令/报告保持原历史形式，不回写。
  - QA happy：`cargo test -p engine --features simulation-diagnostics --test diagnostics`及新增`--test causal_diagnostics`，E/task-36-happy.txt。
  - QA failure：错配订单ID、重复fill、缺报价、取消与日终混淆、报告溢出/无样本精确语义；同命令，E/task-36-failure.txt。
  - Commit：Y；`feat(engine): 扩展公司与NPC因果量价诊断`。

- [ ] 37. 建立真实三宿主与恢复/频率一致性矩阵
  - 实施：新增`scripts/simulation/host-parity.mjs`、其测试、engine确定性trace fixture及各宿主test-driver；驱动真正WASM实例、Server HTTP/WS actor和Tauri IPC边界，不以三份Rust函数调用冒充三宿主。使用step/暂停命令固定输入，发布频率1/5/30Hz、最快三种传输配置；比较canonical权威状态/事件，不直接比较被压缩传输包。
  - 依赖/并行：W6；28、30–32、34、35；阻塞42；与38/39隔离端口/输出目录。
  - References：K1/K7、ADR0010、`apps/server/tests/{publisher,ws}.rs`、desktop actor、WASM worker、`apps/web/e2e/`。
  - Acceptance：同seed与命令在会计月/季/年结、休市公布、部分成交、保存恢复前后完全一致；快进仅改变墙钟；seq区间完整，披露不丢。Tauri GUI自动化受环境限制时仍须真实IPC和应用启动证据，缺该环境则标阻塞而非跳过通过。
  - QA happy：`node scripts/simulation/host-parity.mjs --hosts wasm,server,tauri --seeds 7,11,19 --output .omo/evidence/company-information-npc-intentions/parity`，E/task-37-happy.txt。
  - QA failure：`node --test scripts/simulation/host-parity.test.mjs`检出删事件/错日期/旧WASM/漏restore状态；真实host断连及坏档注入保持原session，E/task-37-failure.txt。
  - Commit：Y；`test(engine): 覆盖三宿主财务与个人状态精确重放`。

- [ ] 38. 完成同seed基线对比与参数敏感性
  - 实施：扩展任务1脚本after分支，以新格式fixture表达同一股票/账户/初始交易资产/时钟密度/seed，不导入旧档；附新增公司初始条件和日历差异说明。保持10seed×30交易日主矩阵，再用小规模四行业400自然日、5seed跨年长局和0.5x/1x/2x行为/事件参数敏感性。C01量能分母作为观测假设单独敏感性，不制造U型成交。
  - 依赖/并行：W6；1、36；阻塞41、42；与37/39分资源运行。
  - References：K4/K5、E/before/、清单C01–C06、`diagnostics.rs`、`docs/diagnostics.md`。
  - Acceptance：同一报告保留每seed原值/分位/极端/无样本及before-after意义差异；新增机制对指标的影响可解释，但不凭调参使所有seed反弹。无授权数据源时C06真实市场留出校准明确未完成，不为本次机制验收伪造数据。
  - QA happy：`node scripts/simulation/baseline-run.mjs after --output .omo/evidence/company-information-npc-intentions/after`和`node scripts/simulation/baseline-run.mjs sensitivity --output .omo/evidence/company-information-npc-intentions/sensitivity`，E/task-38-happy.txt。
  - QA failure：同脚本测试检出不一致seed集合、以旧档加载after、遗漏零成交样本、错误当统计拟合pass；E/task-38-failure.txt。
  - Commit：Y；`test(engine): 验收公司行为多seed分布与敏感性`。

- [ ] 39. 验证独立账户规模和新增状态长期成本
  - 实施：扩展现有`tests/session.rs`scale gates保存/对账个人信息/预期/计划/会计/日历；新增`tests/company_scale.rs`，2万默认5股跨季90自然日、四行业小账户10年归档成本、10万高关注/多计划存档峰值探针。测量推进CPU、内存、存档/恢复、序列化和最大body，必要优化限共享不可变报告、稀疏队列、增量索引，不合并NPC或丢权威状态。
  - 依赖/并行：W6；27、36；阻塞40–42；长跑与37/38避免同机器性能测量互相干扰。
  - References：K2/K4/K7、清单A09、`tests/session.rs`三scale测试、ADR0013成本探针、server资源门禁。
  - Acceptance：20k/50k/100k逐户完整恢复+一致重放，零全局订单上限误拒；2万跨季报告/计划保持；所有资源值原样记录，超过部署上限须优化或显式报告，不偷偷提高阈值掩盖。无硬件无关速度承诺。
  - QA happy：分别运行`cargo test -p engine --release --test session twenty_thousand_accounts_roundtrip_and_complete_a_full_market_day -- --ignored --nocapture`、同命令替换`fifty_thousand_accounts_roundtrip_and_complete_a_full_market_day`和`one_hundred_thousand_accounts_roundtrip_and_complete_a_full_market_day`；再`cargo test -p engine --release --features simulation-diagnostics --test company_scale -- --ignored --nocapture`，E/task-39-happy.txt。
  - QA failure：规模档删除一个NPC状态、篡改报告引用、超过配置body/集合上限、订单索引不一致均明确拒绝且旧局保留；`cargo test -p engine --test scale_restore_limits`，E/task-39-failure.txt。
  - Commit：Y；`test(engine): 验证新公司与个体状态的独立账户规模`。

- [ ] 40. 验证产品release无调试且行为一致
  - 实施：新增`scripts/simulation/release-contract.mjs`和测试，构建真正prod Web/WASM/Server/Tauri到隔离目录；收集feature图、artifact清单、debug能力探针及固定场景摘要。debug/优化诊断构建不可混进分发目录，Rust Cargo feature统一可能带入trace必须在独立命令/target目录构建。
  - 依赖/并行：W6；30–32、34、35、39；阻塞42；独占产物目录。
  - References：K7、package/Cargo manifests、`scripts/wasm-build.*`、three host入口、任务35feature约束。
  - Acceptance：prod无NPC inspector chunk/route/IPC/worker debug能力、无128条trace缓冲采集，正常错误与save私有状态保留；dev、prod、optimized diagnostic相同输入权威结果一致。仅grep字符串不够，必须运行产物并探测。
  - QA happy：`node scripts/simulation/release-contract.mjs --output .omo/evidence/company-information-npc-intentions/release`执行构建/启动/探针；`pnpm build`、`pnpm types:check`、`pnpm wasm:check-threading`，E/task-40-happy.txt。
  - QA failure：脚本单测包含误启feature、旧wasm、混入dev chunk、release debug调用及正常报错误删检出；`node --test scripts/simulation/release-contract.test.mjs`，E/task-40-failure.txt。
  - Commit：Y；`test(web): 验证三宿主发布产物与诊断隔离`。

- [ ] 41. 同步领域文档、游戏简化及完成状态
  - 实施：更新原ADR0016、price-volume清单、ADR0006/0011/0013/0015受影响语义、architecture/testing/diagnostics/trading-rules/open-questions及DESIGN。任务2会计/日历/公司行动文档填入真实实现路径和证据；修正A04/A11、M01和检查顺序过时矛盾。历史V机制标被替代，不抹除历史。
  - 依赖/并行：W6；34、38、39；阻塞42；与40并行。
  - References：本文需求矩阵、E/所有结果、原两份需求文档、`docs/decisions/`、`DESIGN.md`。
  - Acceptance：每项清单明确完成基础/统计待校准/不支持，不将C06实证数据或未实现股东支付标完成；报表合同支持范围、2030日期不预测未来制度、未来日历模拟标签、旧档删除均诚实。
  - QA happy：独立agent逐链接读取实现/测试/证据并按需求矩阵审查，输出E/task-41-happy.txt；不对自然语言句子写匹配测试。
  - QA failure：以“无银行股票”逃避银行报表验收、称未知假日官方、把公司借款误写禁止、把旧历史合成K当实盘成交等必须CHANGES_REQUESTED，E/task-41-failure.txt记录审查判据/纠正。
  - Commit：Y；`docs(engine): 同步公司信息与个体行为实现边界`。

- [ ] 42. 执行全套回归并封存交付证据
  - 实施：新增`scripts/simulation/verify-plan.mjs`只校验机器可消费证据/命令结果/任务数而非文案；从干净任务构建目录跑全门禁。before固定任务1原revision+dirty内容摘要，最终after/三宿主/release/规模/视觉对应本次实施revision；逐批审查标base/head或diff digest，最终审查覆盖完整实施diff。仅添加证据文档的提交不递归使实施证据失效。不得因耗时跳过ignored压力测试却写全绿。
  - 依赖/并行：W6；1–41；阻塞F1–F4；最后执行，不与改代码并行。
  - References：Verification strategy、`package.json`、`docs/testing.md`、E/全部证据、任务37/39/40驱动器。
  - Acceptance：有效行为测试未弱化、零测试匹配不算通过；成功门禁exit0，红灯及故意失败子进程的actual符合预定非零exit且失败原因正确，证据revision遵循上条。缺依赖/环境明确BLOCKED，不能把工具不可用记pass。
  - QA happy：`pnpm test`、`pnpm lint`、`pnpm types:check`、`pnpm build`、`pnpm test:e2e`、`cargo test -p engine --features simulation-diagnostics`、`cargo test -p engine --release --test diagnostic_absence`及`node scripts/simulation/verify-plan.mjs --evidence .omo/evidence/company-information-npc-intentions`，E/task-42-happy.txt。
  - QA failure：`node --test scripts/simulation/verify-plan.test.mjs`针对缺报告、旧revision、零测试、忽略宿主、伪成功日志、缺独立review返回非零；E/task-42-failure.txt。所有失败修复后重跑，不修改成功标准。
  - Commit：Y；`test(engine): 固化公司信息模拟最终回归门禁`。

## Final verification wave

> 所有实施任务之后并行执行。四名独立审查者全部APPROVE；有效发现修复并重新核验，向用户报告后等待确认，不能由实施者自报完成替代。

- [ ] F1. Plan compliance audit
  - 检查用户决策、需求矩阵、42任务证据和完整diff；逐项验证四行业实质、无共同V、跨日计划、日历、无派钱、旧格式删除及三宿主。References：本文Scope、原两份需求文档、E/task-*-review.md。
  - QA happy：审查者逐项读取证据原文件和测试报告，输出E/final-F1.md；failure：缺任一行业/宿主/独立审查或零测试匹配必须CHANGES_REQUESTED，不能跳过。
  - Commit：N；审查文档及后续修复分别记录。
- [ ] F2. Code quality and A-share/accounting semantic review
  - 未参与实施的reviewer审查完整diff，核对官方规则适用日期、报表确认分类、自然日/交易日、金额单位、T+1/委托/费用、边界解析、模块责任、RNG及错误处理。
  - QA happy：读取task2来源正文与金样，复跑相关cargo测试，输出E/final-F2.md；failure：未知规则当默认、无单位f64跨边界、隐藏fallback、超范围临时支持或弱化断言均拒绝。
  - Commit：N；不允许reviewer偷偷实施修复。
- [ ] F3. Real manual QA executed by agent
  - 使用实际构建驱动WASM/Remote浏览器及Tauri真实IPC/应用流程，日期选择→公开财报→下单→跨日→存档恢复；375/768/1280截图、键盘/空态/错误态、dev追踪/release缺席。必须调用visual-qa；PR交付前调用review-work。
  - QA happy：`pnpm test:e2e`与任务37、40的真实驱动通过，证据E/final-F3/；failure：损坏文件、休市日披露、断连/迟到响应、release访问debug均有预期结果；不可用宿主标阻塞，不假称覆盖。
  - Commit：N。
- [ ] F4. Scope fidelity and evidence audit
  - 核对无新增默认股票/股东资金流/旧引擎/隐私工程/无关清理，所有配置变更可追溯，before/after可比并未宣称实证拟合。核对用户原有改动未覆盖。
  - QA happy：复查工作树基线清单、提交范围、删除清单和E/报告，输出E/final-F4.md；failure：缺源码revision、实际命令/退出码/非零测试数、用旧WASM产物测新Rust或隐瞒失败均拒绝。
  - Commit：N。

## Commit strategy

- 本规划阶段不做Git操作、不提交、不push。
- worker开始读`docs/git/AGENTS.md`和git-master；如需要PR/分支，使用任务专属worktree，不在有用户未提交改动的文件上盲写。
- 每任务一个可回滚逻辑单元；测试与对应实现同提交，纯抽取单独提交。文档同步可随对应语义任务提交，禁止一个提交混入无关任务。
- 各任务Commit行是未来建议，非已完成承诺；未过任务独立复核不得标完成。
- 不保留旧引擎为回滚手段；回滚采用代码revision及对应新局。用户存档文件不删除、不覆盖；旧格式不兼容是已批准的破坏性契约变更。
- push、建公开仓库、发版须用户另行确认；GitHub操作只用gh。

## Success criteria

1. 四行业各自实际经营/会计/报表及合并场景通过，五类报表产物来源可追溯且勾稽一致，不只是随机字段或改表头。
2. 2030默认和其他起点日期可选，公历正确，未来模拟休市标识诚实，存档固定日历/政策，休市经营及披露不造交易。
3. 基本面和非基本面/混合NPC均独立分析，无共同V、无前视，预期变化有信息/经历原因，技术历史不足显式表达。
4. 个人计划保持/修订/暂停/终止可解释，跨日不复活旧委托；跨股现金/费用/T+1/部分成交完全对账。
5. 股东资金流只在设计文档，本次无任何派钱/股份变化/除权执行；正常公司经营与商业融资真实记账。
6. 新格式可严格原子恢复，全部旧格式专用代码已按清单删除且现行有效测试未削弱。
7. 公共界面支持完整披露、报告版本/期间/合并范围，三宿主结果一致，dev诊断不影响结果且产品release无调试入口/额外采集。
8. before/after、固定场景、多seed、2万/5万/10万及跨会计期间门禁有原始证据，所有失败明确报告，不宣称未经真实数据证明的统计拟真。
9. 42项任务以及F1–F4全部有独立证据/审查回执后才可向用户提交实施完成报告；当前仅计划落盘不满足这些实施条件。
