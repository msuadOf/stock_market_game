# Sweep27：Task24–26 历史复核逐章追踪

## 范围与阅读记录

- 基线 `4ad5a2e298d086024e6b0069b98b4a6b195a0a01`，按父任务说明其产品树等同 `b76ece3`；沿用已读的根 AGENTS、principles 与现行 ADR-0017/0019。
- 已连续全文读完 `.omo/evidence/company-information-npc-intentions/task-24-review.md`（52 行）、`task-25-review.md`（191 行）、`task-26-review.md`（260 行），合计 503 行，包括初轮 REJECT、事故错误基底、恢复记录及全部二次 APPROVE。
- 本轮静态追踪当前披露、曝光发现、个人获知、策略/计划与执行、保存恢复 caller。没有修改产品、Git 写操作或执行测试/长验收。以下历史测试数值只作为原文记录，不是本轮运行结果。

## 条款族与当前落点

| 历史条款与原文行号 | 当前状态 | 代码证据与解释 |
|---|---|---|
| Task24:21 发现1：可撤阶段与禁撤窗口 | 已保留 | `packages/engine/src/session/plan_execution/actions.rs:203` 依 TradingPhase 判定；Continuous 可撤，CallAuction 依 auction_ticks/3，PreOpen/ClosingAuction 不可撤。仍经过真实 P4 业务规则，不凭计划内部结果释放资源。 |
| Task24:24 发现2：统一订单/预留/结算，没有第二套执行系统 | 历史直接路由已迁移到统一 pipeline | `plan_execution/actions.rs:41` 只准备 `PlanExecutionProgress`；`:71` 准备真实撤单命令。`plan_chain_candidates/adaptive.rs:250` 读取当前计划准备请求；`pipeline/ready_stock_stream.rs:78` 根据对应 P3/P4 typed 结果续行；不再要求恢复已退役 route_intent/route_plan_intent API。 |
| Task24:27 发现3：完全一致工作单认领保留原 OrderId，价格/数量变化真实撤旧换新 | 已保留 | `plan_execution/actions.rs:50` 精确比较 side/price/qty，`:56` 返回 Adoption 并保留 candidate.id；`plan_execution/interpreter/resume.rs:69` 在真实 Cancel 成功后清旧引用、重取当前 remaining 再提交。没有为了旧队列优先而重新发单。 |
| Task24:30 发现4：Accepted 在 Fill 前、全填完成不再被非法日终事件推进，auction→continuous余单身份 | 已有当前事实投影 | `pipeline/adaptive_plan_chain.rs:378`/`:471` 接受对应身份 typed outcome，`:649` 处理 AuctionLifecycleFact；`plan_execution/synchronization.rs:18`/`:29` 映射真实 Accepted/Filled。`plans/mod.rs:421` 只忽略同一已封批中完成之后的后续事实；不是忽略批前终止或未知计划。 |
| Task24:33 发现5：额度不足、子手剩余、不兼容母单、失败撤单不伪释放 | 已保留当前边界 | `plan_execution/actions.rs:24` 依 live_cash_reservation 校验 allocation，`:31` 明确 AllocationInsufficient；`:82` 校验 parent.linked_plan_id；`interpreter/resume.rs:118` 重新核对剩余与版本，不同情况返回 typed Waiting。现有 `continuation_tests.rs:148`/`:216`/`:267` 分别覆盖部分成交换单、完成撤单失败、仍活跃但子单已填业务反馈。 |
| Task24:36 发现6：同步失败原子性、pending瞬态不入档 | 同步原子性保留；瞬态旧契约已被后续SaveSlot契约取代 | `plan_execution/synchronization.rs:51` 调 `apply_active_events_atomically`；`plans/mod.rs:401` 只暂存触及计划，`:431` 全部成功后安装。`session.rs:2692` 将仍存活计划的 pending 事实入档，`:2978` 恢复，并非历史“中途持久化必丢 Fill”问题仍在。 |
| Task24:39 发现7：最小范围、生成类型范围与质量门禁 | 历史流程记录 | 旧 diff/当时 generated dirt 与后续生成迁移不能作为现行产品缺口；本轮只核对当前代码/类型使用，不复述旧测试通过为当前验证结果。 |
| Task24:42 发现8(a)：从未提交子单的计划也须日终到期 | 已接线 | `pipeline/auction_day_end.rs:2185` 的 `sweep_owned_plans` 先同步，然后 `:2194` 取全部 active_plan_ids，`:2197` 对全部活跃计划应用 TradingDayEnded，不以 linked parent 存在为前提。该 transition 同时被连续收盘 finalizer 使用。 |
| Task24:42 发现8(b)：保存前同步或保留 pending，避免 Fill 丢失 | 已有后续存档闭环 | 当前 SaveSlot 保存 plans、个人 information/beliefs/watchlist/price-memory（`session.rs:2664` 起）与活跃 pending（`:2692`），恢复在 `:2959` 起重新安装公司域/计划权威状态及 `:2978` pending。不能要求旧外部 PlanBook 交接协议重新出现。 |
| Task24:42 发现8(c)、Task26:84–94/212–230：终止或反向修订前真实撤子单 | 已修复并迁移 typed continuation | `decision_chain.rs:381` 对 Terminate 发 termination operation，`:416` 对 Restructure 发 operation；`plan_chain_candidates/adaptive.rs:203` 有子单时先生成 `restructure_event`；`plan_execution/interpreter/resume.rs:19` 等真实 Cancel，`:21` 复查 version/fill/status/child，`:31` 才应用事件，`:33` 清 linked parent。旧 helper 的同名缺席不等于回归。 |
| Task25:13–35、39–55：公共曝光只改变发现，实际观察才获知，未公布无前视，個体RNG/全市场发现 | 已有真实机构根 caller | `decision_chain/roots.rs:308` 使用个人 attention.sample_discovery_stock，`:315` 写实际关注；`:349` 从 discovery_candidates 取截至 now 公共材料，`:360` 才 record_acquisition；`decision_chain.rs:149`/`:155` 公开报告/公告查询截点为 now。直接变私有经营账不会从此读取未公布材料。 |
| Task25:43–54：60%/70%分支、2%/2x/boost、异常窗口、protected/上限8与终止可淡出 | 已有模块与机构根接线 | `session/attention.rs:102`–`:114` 对应参数；`:188` 的 select_discovery_stock 过滤真实 market成员并保留持仓均匀与其他加权池；根 `roots.rs:482` 持仓∪active_codes 保护，`:486` prune。终止计划移出 active索引，随后可修剪；没有误用固定业务配额限制订单。 |
| Task25:61–76、109–120：LOC/浮点/QA统计、task18未提交基底口径 | 历史质量证据 | 身份、金额与日期并未因此改为浮点；权重/概率允许f64。旧LOC豁免、计数归因及未提交测试与当前功能是否接线分开处理，不作为额外生产功能。 |
| Task25:80–105/129–131：F1 新游戏参数无fixture/docs登记 REJECT | 已核销 | `tests/fixtures/company-model/policy-sources.json:590` 的 game-assumption-attention-discovery-weights，`:600` 记曝光加成；`docs/company-accounting.md:156` §2.8 保留参数模型。与当前 attention常量一致。F1是登记阻断，原文本明确engine代码无需返工。 |
| Task25:122–127：Task26曝光新鲜度、公司↔股票映射、held池过滤接线 | 已有接线；登记分开 | `decision_chain.rs:131` 通过 issuer_of映射，`:136` 以 EXPOSURE_FRESHNESS_DAYS（`:257` 为2自然日）查公开新鲜材料；生产根消费曝光与个人 attention。`docs/superpowers/specs/2026-09-13-company-information-learnings.md:1233` 明示2日，issues中另有参数登记边界，不能仅旧任务25未登记这个后续值就判新增代码遗漏。 |
| Task25:135–191：错误基底a885ac1与F2血统事故 REJECT，恢复bea13b9 APPROVE | 历史事故已核销 | 当前 attention discovery、information acquisition、fixture条目及根生产调用均真实存在。旧wrong-base树缺文件和当时worktree残留不是当前HEAD行为，不能按事故前缺文件重新开功能项。 |
| Task26:30–75/175–198：误删非V涨跌停、笼子、舍入与显式溢出测试 REJECT | 已恢复当前测试目标 | `tests/market/main.rs:38` 保留market limit stops，`:103` 保留真实成交；`tests/market/price_limits.rs:8` 舍入与最小tick，`:39` 笼子参考序，`:76` 低价范围。历史文件从market.rs拆为目录不是丢测试。没有为旧V API保留不再支持规则。 |
| Task26:77–82/199–210：证据文件缺席、E/重复日志卫生 | 历史证据/文件管理记录 | 当前树无 E/ 产品或日志路径；`.omo` review与其余历史证据存在。旧happy里失效 E/路径属于文档漂移，不构成交易生产漏实现。 |
| Task26:96–106/247–260：终止/未知pending累积和company_assembly体积登记 | pending语义已收口；LOC是维护记录 | `plans/mod.rs:408` 未知显式UnknownPlan，`:409` 批前terminal显式InvalidTransition；仅 `:421` 同批完成后迟到事实消费为空转移；成功后 `synchronization.rs:66` 清pending。失败不静默drain，候选回滚。旧“未知/terminal永久retained”已不适用。 |
| Task26:110–117：删除共同V与跨宿主旧字段 | 已核销 | 搜索当前 packages/engine/src、Web、WASM、Server、Desktop 的 fundamental_value/v_initial/v_params/VParams/TrackV/VError 无实现命中。现有估值在个人BeliefInputs/ValuationOutcome内，不是复活共同V。 |
| Task26:118–128：K4/K5本人的信息集、总发行股本分母、五路信号、Unavailable与能力入链 | 主要链已有，已有缺口仍归总账 | `roots.rs:404` 构造个人 NpcObservationContext，`:422` 使用 total_issued_shares；`:118` 等构造信号，无条目返回Unavailable。当前只将Annual送新材料（`:366`/`:394`），中期补充归 G09；散户分析链归 G07；不能据历史“ActiveTrader不可达”重开，后续已允许它进入计划链。 |
| Task26:129–143：RNG隔离与决策链、日终经营→封账→披露 | 主链已有，跨自由调度字节一致旧断言已被后续契约取代 | `roots.rs:308` 消费个人流，`:476` 生命周期、`:479` AccountExecution；`session.rs:2105` 经营日终、`:2110` 封账、`:2114` DisclosureDispatch、`:2123` prune dispatched。公司行业/合并/支付不完整分别沿用G28/G35/G36，不能以披露调度存在全部核销。 |
| Task26:144–160、232–245：测试迁移、旧全量重建重放恢复过渡、ADR指针、质量结果 | 存档过渡已由后续权威状态契约替代，其余历史记录 | 当前保存company_operations/closing/public_library/wiring/disclosures/plans及个人状态（`session.rs:2659`起），恢复直接装回（`:2959`起）而非当时仅重演经营或剥离 linked_plan_id。全量重跑次数/ignored等只作为历史记录；本轮无新验证。 |

## 新候选反证与现行缺口归并

1. **“计划从未有子单所以永不到期”已反证。** 当前共用日终 transition 显式遍历全部活跃计划，历史 Task24 forward note 不能压过后续真实 caller。
2. **“终止子单不撤、一直成交到日终”已反证。** 生命周期现在发 typed 撤单 continuation；直到真实股票簿反馈才应用终止/反向。取消旧helper名称/直接路由符合后续统一执行决定。
3. **“pending瞬态不入档导致读档丢Fill”已反证。** 当前SaveSlot包含活跃 pending并恢复。未知/批前终止 facts 仍明确失败，同批完成后的迟到事实才被消费，语义比历史retain更严格。
4. **“发现/曝光helper完全测试专用”已反证。** InstitutionalDecisionRoot::observe_personal真实调用 sample_discovery_stock、record_attention、discovery_candidates、record_acquisition；它确实接进 plan根，不是只有模块存在。散户基本面缺口仍按G07，不因机构调用已存在误核销。
5. **“F1/F2 REJECT仍悬空”已反证。** 必须读到Task25:191与Task26:256后的最终结论，并核当前模块、登记与市场测试；原错误基底和初轮误删已修复。
6. **“历史APPROVE证明全链全完工”不成立。** 当前新材料只分发年报归G09；诊断 root提前记录而非真实订单关联归G37；预算新机会分类归G38；个人读取记账边界归Q02，不能被旧11条集成测试与APPROVE掩盖，也不另重复编号。
7. **“same_seed跨worker必须事件字节一致”是过时验收。** Task26:133旧断言被后续允许实际并发受理差异的ADR-0017及K7收口契约覆盖，现行工具仍错误比较artifact的问题已列G39，不借旧复核要求恢复跨实体全序。

结论：本轮503行历史复核没有确认总账之外新的代码遗漏。Task25登记/血统事故与Task26非V测试误删均已核销；Task24移交的日终、终止撤单、pending保存与清理已有当前生产链。已知领域/诊断缺口保留G07/G09/G28/G35/G36/G37/G38与Q02，不重复计数；没有宣称本轮测试通过或历史性能数据仍有效。
