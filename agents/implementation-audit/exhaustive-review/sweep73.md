# Sweep73：Session 实施、leaf 与 lifecycle 工作记录全文复核

## 覆盖与基线

- 全文连续读至 EOF：`agents/oop-refactor-implementation/session/implementation.md` **44 行**、`leaf-review.md` **49 行**、`lifecycle-review.md` **35 行**，共 **128 行**。
- 源码目标 `b76ece3`；与当前后续文档HEAD比较，session、plans、ZiNoise相关生产源码无变化。根AGENTS/principles与正式交易规则优先，工作记录中的“行为保持/动作完成”只核销对应owner/caller要求。
- action-index/relationships/action-ledger/source-manifest及validation JSON未纳入当前可见审计worktree；现有Markdown承诺可逐条核对源码，历史数量/运行结果不冒充本轮实际阅读日志或重跑。断链仅记证据边界，不凭此新开产品功能。
- 本轮只写此审计记录，未改产品、未运行编译/测试/长验收、未Git写或访问官方网页。

## 逐章条款矩阵

| 原文/条款 | 当前生产方法及真实caller | 结论与保留边界 |
|---|---|---|
| implementation引言 1–5：19主动作及cursor增强全部实施，owner/caller账 | 以下各receiver均在当前源码，定义和caller逐条检查 | 源码可核；历史19项完整action-index缺席，不能声称本组重建所有原动作正文；可选子目标按记录明确项目核对 |
| 已保留边界 7–9：唯一CommittableSessionState、facade poison、shadow/commit释放、不Deref | `session.rs:1125`GameSession持poison/test hooks/state；`:1135`单State；`:1328`clone_for_tick_shadow清facade失败元数据、`:1339`commit只提交state | 实际核心对象已落地；`state.poison`旧错误fixture已修。ProtocolSession的只读Deref不是GameSession偷加Deref，不混为同一条款 |
| 同章 10：CandleBook完整history/active、save/snapshot/hash双字段 | `candles.rs:92`单owner两map；`session.rs:2585` / `:2590`save投影、`:2811` / `:2819`restore；`snapshot.rs:217` / `:227`、`hash.rs:108` / `:109`消费独立两字段 | 真实生产读取/恢复已有；行为窗口截取不等于截断权威历史。Web分时G10/G11仍独立，不能因engine日K完备核销 |
| 同章 11：四成员个人状态、COW、独立attention、旧wire maps/duplicate残态 | `personal_state.rs:10`四成员、`:69`take先attention、`:90`install；`:99`重复时只换watchlist再panic；`session.rs:1147`attention仍独立，belief_participants为AccountPagedMap | owner已接root真实take/install；旧重复失败残态有保护，不把异常duplicate变可恢复业务。SaveSlot/hash相同事实只改内部归属 |
| 同章 12：封存观察、RootReadContext一次capture、单账户root、反馈后重新lifecycle | `plan_chain_candidates.rs:304`capture一次Arc共享→`:325`InstitutionDecisionRoot.observe；`adaptive.rs:162`用observation.observe的封存资源，`:163`从当前PlanBook重新collect；`decision_chain.rs:847`PlanLifecycleReview.capture | 真实接线已有，非无caller对象；读取封存资源和当前计划各有目的，不能机械强求都来自live或都来自旧snapshot。RootReadContext依然plans.clone的性能缺口G16未修 |
| 同章 13：Coordinator个人typed结果、三个上下文map、generation/retry/reconsideration | `plan_chain_candidates.rs:250`Coordinator；`:374`personal.install后`:383`返回generated；`:107`StockRouteCoordination三个map、`:169`remember reconsideration，adaptive按blocked partition并交还action | 实际生产路由/增项仍存在；原roots发现多余append被撤的记录不能据此要求再加重复append |
| 同章 14：TradingPlan/ParentOrderPlan getters、receiver生命周期、分/股/T+1/费用/日终不变 | `persistence.rs:1416` / `:1422` / `:1428`getters；`execution/records.rs:35`matching fill receiver；`persistence/v2.rs:630`fee audit；`snapshot.rs:131`live reservations | 所列接缝已消费；不是新交易政策。进一步法定规则完整度由trading-rules登记简化，不能以refactor完成宣称全部交易边界已有 |
| 同章 16：沿用官方依据未重联网 | trading-rules/ADR已有官方链接、日期与中国结算访问限制 | 证据声明诚实；本轮不更新官方核验日期 |
| 独立复核 18–24：核心/leaf/生命周期/caller/roots发现修复 | 下方有效发现逐项查当前方法及fixture；核心participant测试 `personal_state.rs:138`实际存在 | 旧阻断状态不能继续当现行未修；其它review只按记录引用，不代替它们完整原diff再签署 |
| 实际验证边界 26–32：未行为红、集中编译、63源静审、不完整回归 | 当前源码test及caller可查；历史running阶段被下面final证据段推进 | 不将旧“正在执行”摘出来判当前未完成；没有本轮编译/行为红结果，未声称63文件diff独立重新审完 |
| 最终集中证据 34–38：232case/增量/EPERM原断言重跑、多核deadline/default+feature checks | 测试定义与当前closed findings对应；实际JSON本worktree缺席 | 历史结果不重新验算，不把首次39/41改写全绿；没有要求恢复长回归/发布测试链。日志补齐属证据归档 |
| 最后无用入口清理 40–44：run_chain_for_account无caller后删wrapper、原测试保留 | 当前全engine搜索未见run_chain_for_account定义/caller；生产 `plan_chain_candidates.rs:325`直接InstitutionDecisionRoot；root相关原测试仍用observe/capture | 包装器退休已有反证，不能作为“旧同名API缺失”重开；旧2case重跑及最终SHA只记历史汇报，不本轮背书 |
| leaf范围与方式 5–9：plans/strategy/opening/save/snapshot范围、只静态 | 下列leafreceiver/caller均存在；OpeningFigures `company_assembly.rs:74`真装配消费 | 本组核当前明确承诺，不声称完整重读该历史14文件baseline diff |
| leaf有效发现 11–15：poison误路径、TradingPlan漏getter、ZiNoise全投影缺test | `v2_tests.rs:1307`当前session.poison；`persistence.rs:1416`至`:1446`plan/linked getter；`zi_noise.rs:296`完整非默认投影case | 三项现行源码已修。不能只引用初审“待修复”重复报缺；ZiNoise test覆盖strategy_data而非旧DTO自身往返 |
| leaf三门 17–23：计划版本旧边界、风险latch/阈值、RetailContext、fee/reservation/opening | `plans/revision.rs:244`version checked后失败位置保留；`RetailDecisionContext`实际被retail策略；`CumulativeFeeAuditV2:679`买nominal相等/卖分量上界；`snapshot_inner:131`采集两簿reservations；OpeningFigures装配如上 | 调整归属主干成立；VersionOverflow原局部部分写入是明确保留边界，见下文。累计fee不能反推逐笔收取优先级，不错误新增该能力 |
| leaf修复后复核 25–35：getters/strategy投影/Account/Position恢复 | 非默认ZiNoise `:297`至`:325`逐字段bits/profile；`account.rs:141`恢复balances、`:152`restore_strategy；v2账户恢复先暂存后安装 | 当前调用与test存在；正式完整slot验证在构造应用前，不能要求每个恢复receiver重复check破坏首错 |
| leaf最终caller增量 37–41：ParentOrderPlan::from_saved_facts、双Option独立、getter断言 | `v2_tests.rs:53` / `:524`from_saved_facts；`:546`active_child_remaining_qty；`execution.rs:140`paired writer但恢复`:149`允许独立事实 | 所列compile迁移已存在；不能zip两个Option静默丢掉非法fixture或改变strict-save拒绝边界 |
| leaf冻结SHA 43–49 | Markdown保留文件SHA/diff SHA及限定静态签署 | 本轮未重算完整diff/source SHA；不声称原签署失效或额外运行测试 |
| lifecycle范围依据 5–9 | parent执行、protocol、candles、attention及adjacent生产消费如下 | 原制度范围已有ADR0011/15/18；旧动作索引缺席不能以摘要自动授权扩实现 |
| lifecycle三门 11–15：matching真实fill、linked/unlinked、50股余量、K事实、attention、protocol rollback | `execution/records.rs:7`结算后typed fill→`:35`receiver；`pipeline/adaptive_plan_chain.rs:1633`真fill交付；`candles.rs:151`首成交reset；`attention.rs:263`heap→`:324`pop_due；`protocol/civil/session.rs:9`state共同checkpoint | 主干真实接线，非仅测试调用；adapter fixture不是完整撮合实跑；50股余量仍股，不凭此补额外买入 |
| lifecycle L1 17–21：paired child writer缺定义 | `execution.rs:136`replace_active_child→`:140`replace_child_facts、`:145`恢复接口消费 | 已修，原双事实赋值，无新增guard |
| lifecycle L2 22：continuation测试跨module字段读 | 当前continuation/continuous_tick_transaction相关读使用getters，原测试名称仍存在 | 当前漏getter初审已修；不能由同名Save DTO public字段误判仍跨module私有读取 |
| lifecycle L3 23：未关联母单反向替换缺短矩阵 | `execution.rs:399`测试通过真实materialize、`:420`submission、`:422`typed settlement fixture；转新Sell目标检查filled/child/price/expiry和唯一intent | 所要求短矩阵现在实际定义存在；不声称本轮运行，也不宣称完整订单簿/账户撮合已被此fixture验收 |
| lifecycle 25–27：另组continuous test caller及集中验收待办 | `continuous_tick_transaction_tests.rs:511` / `:562`使用filled_qty()/active_child_order_id()；implementation末段记录后续统一编译/check收口 | 原静态提示不再作为未修候选；完整类型检查执行证据仅历史汇报 |
| lifecycle最终签署 29–35：18文件最终SHA、仅身份/版本补录、不产品改动 | 原markdown签署内容存在，现生产caller与L1/L2/L3闭环一致 | 无本轮重新哈希/动态验收，不把静审者身份补录冒充亲自测试 |

## 明确残余、正式要求与反证

### R01：RootReadContext的PlanBook复制仍归既有G16

- implementation `:12`仅读一次封存事实的对象归属实现存在，但 `decision_chain/roots.rs:25`仍 `session.state.plans.clone()`；Arc共享同一个副本不消除全历史复制成本。
- 这是旧G16同一生产路径，不新增重复编号；不通过重构已完成的叙述核销正式长期状态性能要求，也未测量现行实际wall幅度。

### R02：TradingPlan VersionOverflow原局部部分写入保留

- leaf-review `:19`明确“既有VersionOverflow非原子行为保持”；当前 `plans/revision.rs:239`至`:243`先写direction/target/opinion/confidence/urgency，再`:244`checked version，overflow会留下这些局部修改。
- 此记录没有承诺修复该旧边界。实际交易step有私有candidate/P9隔离，不能只看裸plan方法便宣称默认引擎失败已部分提交；反过来也不能把库级裸receiver称任意拒绝字节不变。
- 作为已知库级边界保留，不在本批升级默认交易缺口；若正式新需求要求裸PlanBook API也原子，需要单独查授权与调用边界。没有为行为保持任务顺带改动它。

### R03：duplicate personal.install的异常残态不是正常重装能力

- implementation `:11`和当前 `personal_state.rs:91`/`:99`确实保留attention先错、watchlist替换后panic旧成员未改的原顺序。
- 这是内部“不得重复install”的不变量失败边界，正常root独占take→install生产路径存在。未证明公开restore或root会合法重复install，不能按panic分支自动提出rollback/兼容重复安装需求。

### R04：日K、publication cursor与attention归属不补齐策略功能缺口

- CandleBook保留完整OHLCV真实来源，但UI分时分钟/跨日是G10/G11；COW个人BeliefParticipant与InstitutionDecisionRoot接线不能核销Retail基本面/dated消费G07/G08、中期材料G09与主动历史读取Q02。
- ProtocolState共同rollback、PublicationFactCursor同tick续号确实生产使用：`civil/session.rs:242`/`:272`/`:321`成功追加，`:304`先begin同tick再`:307`attach_after；不等于Worker消费背压或三宿主交付缺口G18/G19已经修。
- NpcAttentionScheduler只拥有candidate heap，`:324`通过权威attention过滤stale；scheduled due不代表观察已发生，与独立RNG和本人信息边界一致，未发现需要自动观察的隐藏新功能。

## 候选核销与证据边界

- 五项旧有效发现（leaf三项、lifecycle L1/L2）已找到当前修复源码；L3新增反向短矩阵已有真实materialize/typed fill adapter消费。没有将静态test存在报告为本轮case通过。
- `run_chain_for_account`包装器删除后生产root实际入口仍有caller，不能按旧同名入口缺席列遗漏；GameSession唯一state与ProtocolSession只读Deref不冲突。
- 初审“root正在实施/集中检查正在运行”有后续最终段落收口；历史JSON缺席仅限制当前证据重验，不能据早期时态判源码仍编译失败。
- 本轮不新增交易规则、不补股/现金、不重置本人信念/RNG、不改现行严格日终存档范围；所有旧期望按正式ADR与交易规则范围核对。

三篇128行全文复核后，未确认新增现行产品缺口；既有G16及明确旧异常边界保持，所有本记录列出的初审receiver/getter/test遗漏在当前源码有修复反证。实际全回归、长负载、浏览器/三宿主验收及历史执行JSON重验均未在本轮运行。
