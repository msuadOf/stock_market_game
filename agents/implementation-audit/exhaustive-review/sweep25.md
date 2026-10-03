# sweep25：分析档案、个人技术记忆与规则冻结历史复核

审计产品源码 `b76ece3`（worktree merge产品树等同该提交）。沿用已读 `AGENTS.md`、`docs/principles.md`。本轮只新增审计文件，没有产品修改、Git写操作或测试执行；下述历史测试结果只解释历史证据，不宣称当前执行通过。

## 全文覆盖

| 文件 | 连续全文范围 | 行数 | 章节覆盖 |
| --- | --- | ---: | --- |
| `.omo/evidence/company-information-npc-intentions/task-17-review.md` | 1–110 | 110 | §0 diff、§1.1–1.8八项契约、§2最小范围、§3边界、§4四项偏离、§5日志、§6观察、§7结论 |
| `.omo/evidence/company-information-npc-intentions/task-19-review.md` | 1–93 | 93 | §0八条验证/环境失败、§1技术与记忆、§2最小范围、§3边界与绑定、§4五项裁决、§5残余风险 |
| `.omo/evidence/company-information-npc-intentions/task-2-review.md` | 1–26 | 26 | 法源、blocked诚实性、K1/K4契约、ADR、测试及三项非阻塞发现 |

共229行，均连续完整读取。历史APPROVE只证明对应提交范围，不替代下面的当前生产核对。

## Task17逐章条款映射

| 原文 | 当前代码/后续决定 | 状态 |
| --- | --- | --- |
| 11–21、73–76：当时纯新增类型与工厂，不提前接session | `packages/engine/src/session.rs:1875`现按Inst身份装配，`:1893`实际derive_analysis_profile；`session/decision_chain/roots.rs:74`取权重、`:95`blend_candidate | 后续机构装配已实现，历史“会话层零调用点”现已过时；G07仍存在：Retail未因拥有可构造档案就获得生产五路分析链。 |
| 27–34：13风格×5权重、总10000 | `strategy/factory_profiles.rs:19`完整表；`strategy/analysis_profile.rs:118`总和、`:127`声明顺序 | 表与纯类型仍实现，不把表中Retail能力当成Retail实际消费反证。 |
| 36–45：主导风格不重抽、非零逐项0.6–1.4、零不抽、最大余数/声明顺序 | `factory_profiles.rs:53`输入profile、`:59`五项采样、`:87`最大余数、`:128`倍率、`:134`零早退 | 已实现；正常工厂raw值受权重/倍率约束，不消耗原交易RNG。 |
| 47–56：手算金样、方法链接、Bank/Insurance ROE、零基本面无方法、EquityRoe不得入槽 | `analysis_profile.rs:157`方法/权重双向校验、`:181`行业规则；`factory_profiles.rs:142`机构风格及稳定AccountId奇偶 | 已实现纯契约，四行业会话装配另有G36，不能混为方法映射缺漏。 |
| 58–62：严格恢复、无common-V复活 | `analysis_profile.rs:143`serde DTO入口、`:214`deny_unknown_fields；`session/persistence/v2.rs:228`运行态验证；`apps/web/src/save/schema/personal/beliefs.ts`档案字段解析 | 完整恢复校验已有；个人预期通过BeliefBook进入五路候选，未发现此批重引共同公允价。 |
| 64–67：历史未接RNG且回归位相保持 | `session.rs:1887`独立derived_stream("analysis-profile",id)，`:1895`独立belief-assumptions流 | 后续接线使用独立流；并发未来轨迹不要求跨worker全局字节相同，不能沿用历史单线程测试口径误报。 |
| 80–82：8错误变体、内部分层、边界测试 | `strategy/analysis_profile.rs`保留类型化错误/集中构造；`tests/analysis_profiles/`仍存在 | 内核边界可核；本轮未重跑，历史24/24不证明当前G07已修。 |
| 86–89：奇偶方向、行业规则纯函数、expect、warning band | 当前factory/analysis_profile仍遵守同一选择；后续方法拆到`strategy/fundamental/` | 注册裁决没有取消能力独立要求；warning band是历史任务分工，不是新的产品运行能力。 |
| 91–100、104–110：历史测试计数、诊断精度/采样重复、结论 | 614与585差29明确为后续工商测试，不是本轮验证；纯工厂重复和expect不是功能承诺缺失 | 不新增代码G项。公开归一入口的大数输入可能超出工厂约束，见候选边界说明；未当作当前生产事故。 |

## Task19逐章条款映射

| 原文 | 当前代码/后续决定 | 状态 |
| --- | --- | --- |
| 14–27：worktree消失、Tauri frontendDist缺产物、649/660口径 | 当前`apps/desktop/src-tauri`仍消费Web构建产物；历史导出树stub后能check只排除当时类型错误 | 环境失败不是新产品遗漏；本轮没有造stub或运行cargo。历史并发未提交bank测试不能增加当前通过数。 |
| 31–42：具名SMA20/60、RSI14、半偶、缺样本显式、Cutler接受 | `strategy/technical.rs:1`注明方法，`:85`附近SMA、`:101`附近RSI，`:137`半偶百分比；`observation/technical.rs:103`独立Result指标 | 内核已实现；后续`session/decision_chain.rs:88`用权威历史recent_traded并行构造，`:117`附近调用有界观测；不是仅测试调用。 |
| 44–46：ATR需15根、风险/执行而非方向 | `strategy/technical.rs:152`ATR；`observation/technical.rs:40`风险限定；`session/decision_chain/roots.rs:168`附近technical_signal只消费sma20/sma60/rsi14 | 不从ATR产生方向，契约已满足；原文不承诺ATR必须独立生成风险委托，不能据无消费擅自增加功能要求。 |
| 48–51：无量日排除、连续日序/未来守卫、真实样本数 | `observation/technical.rs:55`未来拒绝、`:61`递增、`:74`缺日；`:84`排无量；`session/candles.rs:51`维护traded_count/recent_traded；生产`decision_chain.rs:88`由已校验完整历史取有界尾部 | 后续缓存优化保留指标所需有效样本与全部有效计数；不能因优化入口不每tick重验全历史认定漏校验。 |
| 53–56：价格记忆上限/保护集合、观察锚点与历史读取分开、回拨/未观察拒绝 | `experience/price_memory.rs:85`观察锚点、`:95`读取记录、`:168`未观察拒绝、`:187`上限修剪；`decision_chain/roots.rs:323`生产观察 | 纯函数已有；**上限修剪没有接生产（本轮新候选S25-01）**；主动历史读取没有生产调用仍属既有Q02。 |
| 58、62–71：分层、全部边界、原观察保持、TDD历史证据 | `strategy/technical.rs`→`observation/technical.rs`→`experience/price_memory.rs`边界仍分开；`tests/technical_memory/memory.rs:59`只直接调用memory.prune | 内核/直接修剪测试不能核销S25-01。本人观察由机构根驱动，Retail链限制仍G07。 |
| 73–77、93：StockPriceMemory/PersonalPriceMemory TS未提交债 | 两文件现均位于`apps/web/src/types/generated/`且经git ls-files确认已跟踪；Web `save/schema/personal/memory.ts:10`解析全部记忆状态 | 本文明确点名的两个绑定已补，不能保留其“当前缺文件”说法；未跑全量types:check，不宣称所有生成物一致。 |
| 83–88：并发计数/文件行数、matches断言、clippy、半偶副本 | 原文已说明边界字段断言有效；当前`strategy/fundamental/mod.rs:271`又有受控半偶副本 | 这是重复实现维护观察，不是未实现产品条款；不得为历史“不复核clippy”扩成当前忽略门禁。 |

## Task2逐章条款及历史发现反查

| 原文 | 当前证据 | 状态 |
| --- | --- | --- |
| 8–12：11官方页面、CAS25/30/33、证监会182、沪深2026规则 | `docs/company-accounting.md:8`正式manifest引用；`packages/engine/tests/fixtures/company-model/policy-sources.json`保存文号/取证日期/状态 | 仅继承可定位历史取证；本轮未网络重新核验，不据历史APPROVE宣称规则目前逐条已实现。 |
| 14–15：blocked不编造、CAS2006归档缺口 | 正式`docs/company-accounting.md:247`附近仍列待证条目，manifest保留blocked_reason/attempts | 法源债如实登记，不能要求在源码里实现无法核定规则；模型简化需按正式范围理解。 |
| 17–19：K1/K4冻结、ADR增补、五结构验证 | `docs/company-accounting.md:16`冻结语义；`session.rs:957`只接受当前simulation_policy_id，`session/persistence.rs:260`恢复同样拒绝旧政策；`tests/policy_manifest.rs:469`结构验证、`:548`拒绝future-without-notice | Q03仍为ID和冻结规则集合关系澄清。没有发现跨政策恢复后静默覆盖路径，不能按缺RegulationProfile同名结构新增G项。 |
| 22：年报3-20+7最晚4-27笔误应3-27 | 正式`docs/company-accounting.md:207`已改3-27；fixture `policy-sources.json:511`仍写4-27，而`:509`仍定义3月20+0–7日 | **遗留文档/fixture算术笔误**，不作为新增生产代码遗漏；当前manifest结构validator不校验自然语言算式。 |
| 23：废止与不再执行词差 | 正式文档仍部分采用“废止”，原文已作为措辞nit接受 | 证据措辞债，不扩大为新制度实现结论。 |
| 24：CAS33统一政策/期间压缩到§26 | fixture `policy-sources.json:289`仍将政策/期间概括为第二十六条；正式文档`:49`也概括条款集合 | 遗留精确引文措辞问题，与G28合并生产接线是不同问题。 |

## 新候选S25-01：个人价格记忆容量纯函数未进入生产

原文锚点：task-19-review.md:53–55；正式计划 `docs/superpowers/plans/2026-09-10-company-information-npc-intentions.md:135`。后续计划`:491`再次要求持仓及活跃计划保护、额外8只，未发现后续决定取消个人价格记忆上限。

生产证据：`packages/engine/src/session/decision_chain/roots.rs:322`构造候选并在`:323`逐股observe_price；候选来自`decision_chain.rs:299`的持仓、全部信念条目、活跃计划及新发现股票。末尾`:482`构造持仓∪活跃计划保护集合，`:486`只调用watchlist.prune。全engine检索memory.prune只找到`experience/watchlist.rs:219`及`tests/technical_memory/memory.rs`的直接测试调用；`PersonalPriceMemory`方法在`experience/price_memory.rs:187`，当前没有生产调用。

影响：本人先后发现/观察超过8只未持仓且无活跃计划的股票后，关注列表可以按既定规则淡出，但price_memory.stocks会继续保存旧观察条目。此处所谓上限是相对于受保护股票数量的上限，不是任意请求配额；不主张裁剪持仓或活跃计划，也不主张删除市场的权威历史。

反证核对：① watchlist与price_memory是不同owner，修剪前者不删除后者；② `session/persistence.rs:1184`对记忆逐条检查股票、价格、时间和高低，没有容量守卫；③ Web `save/schema/personal/memory.ts:14`同样只校验结构；④ `price_memory.rs:17`仍写“会话接线属于25/26”是历史接缝说明，不能代替现行生产调用；⑤ Q02是主动读取留痕，修复该调用仍不会自动补容量修剪；⑥ G07是Retail未消费分析链，已有机构价格记忆也存在本项，不能合并核销。

建议必要验收：真实GameSession/生产根观察先后发现≥9个不受保护股票，确认只留最近8个、同分钟按StockCode稳定破同分；同场保留持仓及活跃计划股票，计划终止后可淡出；save/restore前后容量与锚点一致。已有纯memory.prune测试不能替代生产入口测试。当前结论为静态生产链确认，没有伪造运行时触发结果。

其余边界候选：`strategy/factory_profiles.rs:96`公开largest_remainder_normalize仍先用i64求raw总和再转i128，极大合法非负raw可能超出i64；正常生产工厂权重/倍率限定raw总和很小，未找到用户输入该公开助手的路径。本轮不将其列为新的生产遗漏，保留为公共数学API防御边界备查。

本批新增独立生产候选1项（S25-01）；既有G07/Q02/Q03保持。Task19点名的两个TS绑定已补；Task2年报算式正式文档已改但fixture未改，单列证据债。
