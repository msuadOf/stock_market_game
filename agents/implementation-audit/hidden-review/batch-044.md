# Batch 044 独立复核

## 范围与来源完整性

基线：`43b1aa5`；复核目录：`.worktree/implementation-reaudit`。三份来源均连续读取至 EOF；行数和 SHA-256 与 `scan-plan.json` 一致。历史报告仅作为待核材料，不视为已实施指令或生产能力证明。

| 来源 | 读取行数 | SHA-256 | 章节矩阵与覆盖面 |
|---|---:|---|---|
| `agents/oop-refactor-audit/exhaustive/modules/engine-tests-05.md` | 236 | `592118fe5ed2291ae3a96f035b9cb8ba767a882120d813d3afdf5655b6a0e8bb` | lines 1–8 范围/约束；9–78 fundamental_beliefs：估值 guards、信念端到端金样、每股口径与个人先验；79–178 industrial_accounting：固定资产、经营链、库存/应收子账、VAT/CIT 和拒绝路径；179–228 industry_reports：合并、追溯更正及报表拒绝；229–236 跨文件契约、验证限制。22个 Rust 测试/fixture/入口文件。 |
| `agents/oop-refactor-audit/exhaustive/modules/engine-tests-06.md` | 246 | `14588092dfe98b050385ef1743f3fd792f0b4b86ee557693b8c2ccdde8d41fb3` | lines 1–4 范围/约束；5–44 industry_reports 金样与生命周期；45–94 information_acquisition 获取/视图及拒绝；95–174 insurance_accounting 赔案、守卫、金样与重新计量；175–234 market、price_limits、Money、observations、OrderBook、pipeline_contract；235–246 跨文件关系和未运行声明。23个文件，报告称110个 `#[test]`。 |
| `agents/oop-refactor-audit/exhaustive/modules/engine-tests-07.md` | 255 | `baf4e638761adb1b7432332c64c4d8119de26fb4ea08f4a531f0b43dc28b0ab6` | lines 1–18 范围、ADR状态与模块关系；19–80 plan_allocation 与 TradingPlan；81–140 policy_manifest、协议接受/民事更新/宿主恢复/投影/runtime delta；141–250 披露、排期、前史、纠错和周末路径；251–255 限制及交易语义边界。23个文件，测试文件判 retain、纯支撑文件判 support。 |

基础约束已阅读 `docs/principles.md`：尤其原则1、2、3、9。未核验历史材料中提及的官方会计/交易规则来源，不将材料里的“已核验”表述提升为本轮法源结论。

## 全局章节对照

按当前 `coverage-index.md` lines 256–260 的现行分工，batch 044 来源涉及的当前条目应由以下主题记录判断，而不能因历史测试存在而核销：

| 主题 | 当前总账/复核章节 | batch 044 交叉点 | 判断 |
|---|---|---|---|
| 估值、个人信念与经历 | G06–G09、G16、G28、G35–G38、Q02/Q11；`reaudit-engine.md` | 05 的估值/获取、07 的计划分配 | 估值 gold 不能代替修正估值算法或验证生产消费；保持已有缺口结论。 |
| 会计/披露/行业经营 | G28、G35、G36；相关 Q14、Q17、Q19、Q23 | 05 的账套/合并/重述，06 的多行业报表/保险，07 的披露/发布 | 模块级业务计算存在，不等于真实会话日结、披露和存档消费者已接通。 |
| 交易/分配/协议 | G13–G17、G38；Q21/Q22 等以 ledger 具体归类为准 | 06 的市场、订单簿、pipeline；07 的计划/协议 | 基础撮合和协议测试是输入配置下的证据，不代表所有 A 股板块、生命周期或生产调用边界完整。 |
| 政策/测试与验收 | G21/G26/G39；Q05 等 | 07 的 policy manifest 与“本轮未运行”声明 | 结构校验不证明自然语言法源；历史测试清单不证明当前提交通过。 |

逐项映射已与 `implementation-audit-2026-10-02.md` 的 G01–G68/Q 总账段落及其测试映射表 lines 169–182 对照。batch 044 未发现可据此新增的、尚无编号的独立产品缺口；以下局部测试边界均落在已有 G/Q 描述内。未逐项重审与本批主题无关的所有 G/Q 的产品实现。

## 证据与结论

| 历史原文证据 | baseline 43b1aa5 生产 owner / 调用消费者 | 结论与登记 |
|---|---|---|
| `engine-tests-05.md:34`：盈利倍数与现金流金样声明独立手算；同一行明确每股中央锚 1782/1916“当前无显式断言”。`engine-tests-05.md:231` 区分真实披露链与抽取层直测。 | 估值 owner `packages/engine/src/strategy/fundamental/valuation.rs:156-170`；机构消费链见 `packages/engine/src/session/decision_chain/roots.rs:430-447`（历史现行复核也列该 caller）。显式现金流在 159–164 行每期仅除一次同一 discount，终值才重复折现。 | 金样独立性及披露链不能核销 G06 数学缺口；每股中央值断言不足是已有验收边界，不另重复编号。G06、G09 需要的中期材料测试已在总账测试映射 lines 169–171 登记。 |
| `engine-tests-05.md:179-187`：合并 scope 有单一金样；`engine-tests-05.md:233` 称报表覆盖合并算法。 | 日终发布 owner `packages/engine/src/session/disclosures.rs:182-191`，实际请求固定 `ScopeId::Standalone(member)`；合并算法存在但不由该调用构造公开集团 scope。 | 纯 consolidated 报表金样不能证明生产披露接线；对应 G28 已登记，非新遗漏。 |
| `engine-tests-05.md:79-88,169-177`：折旧及税务单元金样，税率明确是合成 fixture；`engine-tests-06.md:3` 声明未查官方来源。 | 工商日常 owner `packages/engine/src/company/operations/industrial.rs:85-100` 进入产销流程；日流顶层见 `company/operations/day.rs:79-85`。底层会计能力与会话周期结账不是同一调用。 | 不能由单元能力推断折旧/所得税/商业债务支付已进入工商期末闭环。G35 与 Q23 已涵盖；材料不支持现行法源断言。 |
| `engine-tests-06.md:15-23,95-174`：四行业报表与保险合同测试；`engine-tests-07.md:253-255` 明确没有生产实现且披露 18:00 是游戏时点。 | 默认上市公司 owner `packages/engine/src/session/company_assembly.rs:331-346` 固定 `CompanyKind::Industrial`；保险运营的确定性赔案日程 `company/operations/insurance.rs:62-76` 以 elapsed 周期记录/支付，但无 coverage_end 条件阻止期后新赔案。 | 四行业局部账套/合约守卫不证明自定义公司 session 组装或保障期后赔案调度完整。前者在 G36/Q19 范围，后者对应 G59；均已有登记。 |
| `engine-tests-07.md:31-39`：分配 gold 覆盖请求次序、风险减仓与 continuation/new opportunity 排序；`engine-tests-07.md:71-78`：计划生命周期测试覆盖订单接受和真实成交的分离。 | 排序 owner `packages/engine/src/plans/allocation.rs:115-125` 有 `ExistingPlan`/`NewOpportunity` 两种优先级；生产 caller 是否实际传入类别须另查 session 调用。当前 ledger 的 G38 记录生产请求均构造 `ExistingPlan`。 | 算法支持类别不等于生产分类已接入；G38 已记录。A 股 100 股约束只限相应明确 fixture，不代表跨板块规则验收。 |
| `engine-tests-07.md:81-89`：policy_manifest 只做机器结构校验，自述自然语言规则另需审查；`engine-tests-07.md:251-255`：未运行测试/构建，也未重查官方规则。 | policy manifest 属验证工具；不能从 engine 业务测试调用链推导法源可靠性或 CI 通过状态。 | 历史文档证据的界限清楚，没有旧结论被新产品代码反证的材料。全程未运行测试或 Git 操作。 |

## 未另立编号的测试边界

- 05 的 central per-share 金样值明确仅为手算锚，端点才有断言；可在 G06 对应修复/验收时纳入独立金样，但不构成新产品缺口。
- 05/06 多处“成功/失败路径”是组件或集成测试 target；若检查会话、披露、恢复入口，仍须沿消费者确认状态和错误出口。G28、G35、G36 已记录此层次差异。
- 06 的 price limit fixture 明确为通用昨收、10% 与 1 分 tick（lines 185–188），自身声明不代表按交易所/板块/证券类别的规则验收。旧材料没有将它夸大为完整法源审查。
- 07 policy manifest 和自然语言法源明确分离；历史“测试证据保留”只表示留存断言，不是本轮运行结果或语义背书。

## 完整读取记录

三个来源均读到物理 EOF；清单声明行数、实测 `wc -l` 与 SHA-256 均一致。全文检查期间有工具输出截断提示，随后按来源分段重读（05 全236行，06 全246行，07 全255行）；无哈希/行数异常。只新建本记录与 JSON，不改产品文件，不运行回归、不写 Git。
