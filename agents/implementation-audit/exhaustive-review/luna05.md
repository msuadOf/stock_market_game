# 三份策略决策全文实现复核（luna05）

- 基线：08e4fc7；当前审计工作树 HEAD：a7c7ce3。仅静态审计，不改产品、不运行测试、不做 Git 写操作。
- 依根 AGENTS.md 与 docs/principles.md 执行。按 EOF 连续通读 ADR-0006（244 行）、ADR-0021（111 行）、ADR-0026（71 行），共 426 行；不是抽取片段阅读。章节矩阵覆盖三文的所有标题与末尾内容，判断以 ADR-0006 2026-09-11 状态更新、ADR-0021 对 ADR-0022 的交接及 ADR-0026 为准。
- 结论：旧 G07、G08、G38 复核仍成立；机构 ADR-0026 成本经历、账户风险暂停及真实成交投影已连接生产。未发现应独立新增的 G 项。没有重新验证交易所法源或运行时行为，不据此声称相关测试通过。

## 全文章节矩阵

| 文档完整章节 | 逐章实现追踪与结论 |
|---|---|
| ADR-0006 Context（14–34） | 账户独立性、同类参数差异、三策略背景。工厂按实例构建 Retail/Inst/Hot（strategy/factory.rs:17-23,47-165）；NPC 决策并行按账户运行（session/pipeline/npc_decisions.rs:160-215）。参与者状态未按类型合并。 |
| §1–§4（37–67） | engine 内策略实现位于 strategy/；Strategy 读取市场/自身视图并返回 Intent；参数采样在 StrategyFactory，运行状态经 StrategyState；玩家返回 None（strategy/factory.rs:166）。注意力与每户运行链在 session。实现目前以 sealed ProductionStrategy + StrategyState 注册（strategy/state.rs:6-35,91-135），这是持久化和权威状态约束下的扩展边界；没有足够证据认定 ADR 的“可插拔”承诺失效或产生行为遗漏。 |
| §5（68–76）及 2026-09-11 更新（231–244） | 共同隐藏 V、TrackV 已明确被公司公开信息与个体判断替代；当前根从个人信息、issuer 与公司账簿装配判断（session/decision_chain/roots.rs:403-425）。未将历史 V 文案误报为实现残留。 |
| §6–§8（77–104） | 玩家不持策略、默认参与者与风格由工厂构建。生产人口和策略身份按 session 建户；积极机构是机构账户但使用动量族，与决策更新后的 ADR 文意相符。 |
| Research / Alternatives / Consequences / Related（105–142） | 作为设计依据和历史取舍复核；“新增策略零侵入”须结合 sealed 状态注册理解，没有将研究映射当成交易所制度依据。 |
| 2026-09-09 下跌价格发现（143–166） | NPC 主动报价选择及个人止损遵循可卖量/T+1；现有 retail 策略经 retail.rs 产生 Highest/Lowest 符号限价，噪声策略调用处见 strategy/zi_noise.rs:149-209,239-283。交易接受/解析在 account validation（session/pipeline/account_validation.rs:595-596）。 |
| 个体阈值与量价反馈（167–189） | Retail 阈值在实例创建时采样（strategy/factory.rs:68-110）；机构政策与信念在新局创建中用账户派生的独立随机流（session.rs:1887-1918）。MarketView 中相对量能、盘口失衡为只读输入（strategy/mod.rs:70-104）。 |
| 订单生命周期、预算、注意力（190–230） | 生产决策由到期候选集合驱动，NPC 独立状态在快照/结果中按账户提交；机构计划报价入口位于 session/decision_chain/quote.rs:3-230，真实费用与合法数量预算在 decision_chain.rs:909-995,1212-1261。资金保留/单股上限已由 ADR-0021 覆盖。 |
| ADR-0021 §1–§2（8–37） | 取消单股上限、无公共留底；买卖量和费用由计划链及交易权威层处理（decision_chain.rs:968-978,1212-1243；quote.rs:194-222）。未发现本轮路径按股票数额外限仓。 |
| ADR-0021 §3.1–§3.2（38–87） | 当前符号最高/最低限价沿 ADR-0022 接线，散户/游资的主动路径可见于 strategy/retail.rs:57-114、strategy/hot.rs:35-49；机构仍有价值保护价，计划链 quote.rs:133-140,175-222。实际请求校验受理时解析符号价格（account_validation.rs:595-596）。普通限价的撮合与余量留簿边界由订单生命周期层负责。 |
| ADR-0021 §4–§5（88–111） | 文档自身明确验证范围及官方规则依据；本审计没有重新下载或查验交易所材料，故只确认没有发现策略实现显式改动交易制度，不作法源独立认证。 |
| ADR-0026 用户决定（7–19） | 个体成本候选、账户暂停、真实买入后不利选择集中于 institutional_behavior.rs:75-155；机构 own observation 与风险记忆在本人决策 root 更新（decision_chain/roots.rs:255-304）。HoldOrAdd/PauseAndReview 不直接下单；成本信号进入已有五路计划链。 |
| ADR-0026 游戏假设（20–49） | 参数声明明确为游戏假设；个体策略参数范围及随机实现位于 institution_experience_policy.rs:9-17,133-163。真实失败影响使用登记事件及 20 交易日衰减；机构现行计算见 beliefs.rs:172-205，Retail dated failure 原语也应和机构实现区分。 |
| ADR-0026 二次补漏（50–64） | institution_account_risk_paused 必填持久字段（beliefs.rs:86-100），只在本人观察时更新（:160-169），报价路径读取该记忆并防止买计划绕过；卖出未被买入暂停禁用。真实买入/退出收费、订单去重写入 retail_projection.rs:529-607。存档缺字段验证位于 session/persistence.rs:1229 等。 |
| ADR-0026 边界（65–71） | 代码继续由账户/订单权威层校验 T+1、整手/零股、费用和价格带。未见对暂停行为添加强制卖出、共同现金留底或外部补钱。 |

## 旧条目复核

| 条目 | 本轮结论 | 原文与现行证据 |
|---|---|---|
| G07 | 仍成立，不是遗漏的新 G | 旧总账 agents/implementation-audit/reaudit-engine.md:35-40 的范围是 Retail 未消费个人基本面/五路判断。当前生产 npc_decisions.rs:179-190 对 Retail 调 Strategy 的个人行为链，zi_noise.rs:178-223 委托 Retail 行为，不走 InstitutionDecisionRoot 的五路信念/计划能力。不能因个人策略实例和已实现机构五路链而核销；也不将所有散户必须估值扩大成要求。 |
| G08 | 仍成立；机构一侧不能用来核销散户缺口 | 散户观察仍调用非 dated writer：decision_snapshot_capture.rs:343-348；散户成交仍走 record_fill_with_order：retail_projection.rs:518-528。dated writer 和 failure_influence 内核存在并不代表生产消费者使用，旧总账 agents/implementation-audit/reaudit-engine.md:39-43 判断仍有效。机构 ADR-0026 是另一契约：roots.rs:266-304 本人观察并更新风险，机构成功结算在 retail_projection.rs:529-607 记账。 |
| G38 | 仍成立，边界仍为同一账户内计划分类 | 当前唯一生产 AllocationRequest 构造分别在 decision_chain.rs:968-979,1232-1243，均填 AllocationClass::ExistingPlan；全仓生产搜索没有 NewOpportunity 构造。优先级实现存在不等于分类进入生产。AllocationExperience::default() 不是此条要求，也不应重复给上游已消费的机构经历加惩罚。 |

## ADR-0026 风险与参数复核

- 每账户策略创建使用 institution-experience-policy 派生流（session.rs:1907-1918），并按机构风格调用 InstitutionExperiencePolicy::sample；不属于 BeliefBook standalone 构造器的 midpoint preset。没有发现先前报告可能遗漏的“个体策略参数生产使用默认值”问题。
- 本人观察由 InstitutionDecisionRoot 对其持仓构建 equity、逐持仓 dated initialize/observe，再更新 account risk（decision_chain/roots.rs:255-304）；其调用只在候选账户决策链中运行。风险暂停 latch 保存在个人 belief 存档；institutional_behavior.rs:58-63 读取 latch，quote.rs:27-57 对暂停计划保留不可撤报价或可撤时取消，institutional_behavior.rs:97-149 不生成强制卖出。
- 不利选择要求个人股票经历包含 last_buy_order_id 与 last_buy_price（institutional_behavior.rs:142-149），开局持仓只初始化经历而不伪造买入（roots.rs:266-281）。手续费和是否净获利在真实结算投影中判断，而不是仅比较卖价和买价（retail_projection.rs:561-578）。
- 暂未发现 ADR-0026 有新独立漏接。审计是静态追踪，未核验所有存档/前端 schema 细节或执行专门定向测试。

## 其他候选反证

- ADR-0006 §5 的旧隐藏 V / TrackV 不作为漏项：同文最后更新明确删去该实现，由公开公司信息及个体信念替代。
- “开放可插拔”与当前 sealed ProductionStrategy 看上去字面有差异，但持久化枚举确需显式状态变体；本轮没有用户需求要求外部 crate 插件，也没有识别到行为错误，因此不独立立 G。未来新增策略必须同时注册状态 hydration/profile/factory，而非只实现 Strategy trait。
- ADR-0021 的价格笼子/价格合法性仍在权威交易入口二次校验，计划层使用合法带并允许提交间行情改变后受理拒绝；不能把文档记载的时序拒绝误报为静默改价或语义绕过。
- 无新候选足以独立于 G07/G08/G38 登记。未进行官方大 A 规则重新取证或测试运行。
