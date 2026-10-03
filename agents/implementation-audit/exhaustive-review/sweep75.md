# Sweep 75：Session roots 与续作迁移记录

- 审计产品基线 `b76ece3`；工作树 `4ad5a2e` 为审计合入提交，生产内容一致。沿用已读根 AGENTS/principles，正式决定优先于内部OOP迁移任务。
- 连续阅读全文至 EOF：`agents/oop-refactor-implementation/session/roots-callers.md`9行、`roots-review.md`121行、`roots.md`42行，共172行；首次输出完整，无截断。
- 只新增本记录，未执行测试、编译、长验收或Git写操作。本批未确认独立于既有总账的新代码遗漏；G16/G38及机构与散户经历范围继续明确区分。

## Roots Callers 所有条款

| 原文行号 | 当前实现与判定 |
|---|---|
| 1–4 范围/阅读 | quote/lifecycle getter caller短修，内部owner接线要求，未新建交易规则 |
| 5 编译02十个E0616 | 历史诊断，不能继续当现行编译失败；当前两文件读取已改getter |
| 6 五字段迁移 | `decision_chain/quote.rs:165,167,169,171,172`读取linked_plan_id/child id/limit/remaining/qty方法；`lifecycle.rs:352,354,486,488,490`母单getter真实消费。不是仅添加getter定义 |
| 7 单位/动作顺序 | getter读取没有重定Money分、股数量、撤单/价格规则；正式A股执行校验不因getter私有化而失效 |
| 8 定向格式/搜索 | 历史lint/diff检查，未运行本轮rustfmt；不以历史记录冒称当前构建通过 |
| 9 验收限制 | 不跑cargo/产品test、不镜像tests、root统一验证为当时协作职责；不能从该worker未跑推导整批至今未验证，也不把它当产品缺功能 |

## Roots Review 全章节/条款族

| 原文段/行号 | 当前caller与状态 |
|---|---|
| Metadata/结论 1–7 | b89afb3起根组静态复核通过；不证明本轮短测/完整回归/长验收 |
| 范围 9–25 | A01/N03/N08/N09/N11与N04caller局部范围；personal_state主体/其他组未全审属于证据范围声明，不是实现债；当前roots与个人take/install真实相连 |
| 大A门禁 27–40 | Money分、数量股、bp、T+1/零股/撤单窗口/板块最大量/价格笼/费用未改；CandidateTargetProposal是目标、不是可支付/可卖证明。ADR0026风险暂停买入不强卖/不补钱仍正式优先，不能从root重构引入统一止损 |
| 必要性/结构 42–52 | `decision_chain.rs:64`DecisionChainObservation；`roots.rs:6`RootReadContext、188InstitutionDecisionRoot；`plan_chain_candidates.rs:107`StockRouteCoordination及root coordinator；`lifecycle.rs:6`PlanLifecycleReview；`plans/candidates/targets.rs:81`CandidateTargetProposal均由生产caller实际消费，见后续矩阵 |
| 新validation撤回 54 | `roots.rs:18–25`capture当前accounts.clone而非clone_for_shadow；R02修复反证存在。既有FrozenPlanChainObservation仍校验见adaptive18–24，不能将两种输入范围混同 |
| 函数/观察顺序 56–62 | `decision_chain.rs:681–684`from_observation/capture_for_roots→`plan_chain_candidates.rs:304–326`capture并行root.observe；个人注意力、公共信息、watchlist与风险更新保留；该迁移完成不核销散户dated G08或中期G09 |
| Frozen lifecycle 63–66 | `adaptive.rs:11–46`固定accounts/markets/auction_orders临时交换；续作时当前plans/个人belief保留；`decision_chain.rs:847`capture lifecycle，`lifecycle.rs:26–39`held/equity，112assess_one。未重封本tick资源预算，正式并发受理规则优先 |
| 三map/route 67–69 | `plan_chain_candidates.rs:107–110`三独立map、154take_pending、188retry；`adaptive.rs:348`真实generation路由取出/续作。错误location `plan_chain_candidates.rs:98`route_invariant存在；R03已修，不重开错误上下文漂移 |
| Proposal 70–72 | `targets.rs:89–109`from_score先weight，101股本/u32收缩，108weight.min，109target_share_quantity整手；lifecycle90真实调用、100–108保留weight/qty错误context。它不承担现金/费用或卖出股数结算 |
| 旧test/短测 73–83 | 原54test/getter迁移/保护是历史静态核对；删除旧snapshot.market断言对应旧伪GameSession投影取消，不擅自恢复旧结构；ready_ingress single worker既有测试不等于本轮实跑 |
| R01 85–89 | 不存在self.operations内层append已删除；`plan_chain_candidates.rs:374`安装personal、383返回generated，外层batch汇总，不重开历史编译问题 |
| R02 90 | capture普通COW clone真实反证；撤回新全账户策略扫描是保持旧读取接受集，不代表生产tick不需要账户校验；正式P1与Frozen捕获仍有各自守卫 |
| R03 91 | route_invariant保持原context，当前98定义及pending检查真实消费，已修 |
| R04 92 | 全生产源码搜索无run_chain_for_account定义/caller；实际InstitutionDecisionRoot326生产使用，移除无caller过渡wrapper不等于删掉机构决策链 |
| 增量caller/cfg(test) 94–98 | `decision_chain.rs:741–744`chain_observation_instant仅cfg(test)，`plan_chain_candidates.rs:406–408`accounts wrapper仅cfg(test)；真实捕获681–684存在，测试helper获批不要求生产消费；静态diff检查不冒称编译 |
| SHA表 100–118 | 十文件最终指纹/行数为当时签署证据，后续变动须增量复核；不是永久禁止源码演进，哈希差异不直接判新功能遗漏 |
| root运行义务 120–121 | 编译features/caller/定向短测/既有事务case属于全批验证；本报告未执行；TDD与全量性能/宿主证据继续区分，不生成新的生产API需求 |

## Roots Implementation 所有章节

| 原文段/行号 | 当前caller与状态 |
|---|---|
| 范围/依据 1–7 | 所属owner/caller迁移，personal_state及plan/execution由其他组，正式ADR与交易规则优先；不将小组任务扩大到全部root产品行为重写 |
| Observation 9–11 | `decision_chain.rs:64,73`一次封存观察，681调用；tick/minute/phase校验仍捕获入口，不是计划对象伪成交 |
| RootReadContext 12 | `roots.rs:7–14`COW accounts、Arc公司registry/operations、library、PlanBook和时刻；没有GameSession/market/history/个人maps/OrderIdseq。但25复制整个PlanBook，G16仍在，Arc包裹副本不消除历史线性复制 |
| InstitutionDecisionRoot 13 | `roots.rs:188–221`单账户personal→observe→typed批次/diagnostics；根worker不直接routing，当前plans从context.plans读，后续execution按正式边界走 |
| Coordinator 14 | `plan_chain_candidates.rs:304`先capture共享ctx再311take个人、324spawn、325observe、328先send结果再330notify、362yield、374install；内部owner消费完整，不是空结构；通知发送失败代表receiver已关闭/私有tick丢弃，不直接视作用户错误静默吞掉 |
| Lifecycle 15 | `lifecycle.rs:6,20,38,112`派生held/equity、当前plan/parent/belief及零equity先返回；adaptive固定资源/当前计划边界已有。预算新旧机会分类G38仍未由此迁移补齐 |
| StockRoute 16 | 独立三map/错generation先检查、reconsideration优先不删retry，当前owner107、take_pending154以及adaptive348caller存在 |
| Proposal 17 | targets81/89与lifecycle90，收缩+整手顺序已有；参数目标不是订单接受结果，正式A股限制由执行/撮合负责 |
| 领域/事务保持 19 | ±2000bp、本人风险、T+1等旧方向保持；P9commit/候选丢弃与日终存档有独立正式契约，不以root纯观察替代事务提交 |
| 测试方法 21–23 | worker未cargo及精确rustfmt为历史工作流程，不能自动认定现行缺测试实现或本轮全绿 |
| 六新增filter 25–32 | proposal/root_coordinator/stock_route/时钟/观察顺序/fixed-resource-current-plan用例源码已在既有review列明；本轮只追生产caller、不重复造镜像测试 |
| 既有保护 34 | personal owner、无position/belief、childfill pending、cancel rejection、整手/费用/risk等仍正式保护；新增内部receiver不代替全部accepted-set穷举 |
| 修正 36–38 | 内层append、route location与ParentOrderPlan私有字段caller已修，当前getter消费如上；不能保留旧E0616当现行错误 |
| R02/cfg(test) 40 | RootReadContext普通accounts.clone与test专用wrapper范围已有反证；Frozen既有validation留在自身捕获，非新增错误接受集合 |
| 当时等待 42 | 实施记录当时等待root冻结/验证不是现行产品漏实现。后续roots-review:6静态通过、domain/session全批最终证据由父审计结合，无本轮编译冒称 |

## 残余与反证结论

- **G16 保留**：`roots.md:12`描述“只读PlanBook”是权限/所有权职责，不承诺消除深clone；实际 `roots.rs:25`完整PlanBook clone，经 `plan_chain_candidates.rs:304`每批root捕获。在Arc内共享仅减少各worker重复副本，不满足正式Sept24计划普通tick不随长历史线性复制目标。未测量性能幅度。
- **G38 保留**：root/lifecycle/candidate owner迁移不改变分类输入，`decision_chain.rs:972,1236`两个生产预算请求仍 AllocationClass::ExistingPlan，正式公司K6新机会/旧计划预算区分缺口保持。不扩大成跨账户撮合优先。
- R01–R04以及私有字段E0616有当前反证全部关闭；无run_chain_for_account wrapper不意味着实际root chain缺失。
- 当前同批固定账户观察、当前candidate PlanBook、typed outcome后续作都有caller；不从冻结资源要求推导以后tick资源永远不可变化，也不要求恢复旧GameSession伪投影。
- 本批未发现须追加新G的独立遗漏。静态重构通过/方法存在不核销其他组产品能力遗漏；完整回归、性能、多核负载和真实宿主矩阵仍需独立运行证据。
