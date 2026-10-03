# sweep71：pipeline stream 与 state callers 后续工作记录

日期：2026-10-03。生产源码基线 `b76ece3`，审计 merge `4ad5a2e` 的生产代码相同。已读根 AGENTS/principles，沿用现行 ADR-0017/0018 的实际局部受理与私有tick契约。只新增本记录，未修改产品、Git状态，未运行编译、测试或长验收。

## 全文阅读范围

| 指定原文 | 全文行数 | 已读章节 |
| --- | ---: | --- |
| `agents/oop-refactor-implementation/pipeline/review-stream-callers.md` | 77 | 范围/35文件清单、大A语义与时序9项、必要性、S1及修复、当前结论、编译02 getter、编译03 Position比较、最终SHA门禁 |
| `agents/oop-refactor-implementation/pipeline/state-callers0.md` | 44 | 范围与结果、15文件清单、语义/验证、独立复核补修 |
| `agents/oop-refactor-implementation/pipeline/state-callers1.md` | 71 | 范围、迁移内容、验证边界、36文件版本表、非空belief_patch补充、精确filter和中间指纹 |
| 合计 | 192 | 三文均连续全文读取，无省略。 |

补充读取当前 `session_execution_transaction_tests.rs` 全文（266行），核对生产stream/receipt/participant写回、getter、attention owner与最终review manifest。下文“已有”是静态代码存在；历史运行证据另注明来源，不能当成本轮重新执行。

## review-stream-callers.md 逐章与时序条款

| 原文位置/条款 | 当前状态与代码证据 |
| --- | --- |
| 3–19行：限定35文件、32caller与3stream文件、其余分配表无diff不计 | 最终manifest仍为35条；本轮只读SHA-256复算所有条目，35/35与当前源码一致，漂移0。不存在根据旧 `/tmp` 分配文件数要求重做72个产品文件的承诺。retail_projection_persistence_tests已转交projections，不能继续算本caller组漏实现。 |
| 23行第1项：先payload再Stock notification，两channel不要求同序 | `pipeline/stock_stream.rs:360`先运行私有shard，`:364`发送payload成功后`:365`通知；notification无stock身份，`:396`从结果channel消费已完成payload。跨worker通知顺序不强制变成交易优先级。 |
| 24行第2项：in-flight校验、一次回收、错误传播、关receiver丢私有结果 | `stock_stream.rs:402`移除in-flight并拒duplicate book，`:405`传播round错误；`:363`明确整个tick丢弃后channel关闭只丢工作。不能增加已丢弃tick的错误回传要求以恢复旧工作流。 |
| 25行第3项：coordinator宿主线程、单worker帮助执行 | `stock_stream.rs:430`仍in_place_scope；`:373/384`单worker等待时yield_now。无多宿主coordinator占满Rayon的新增等待层。 |
| 26行第4项：过时PlanRoot只轮询，不消费stock payload | `stock_stream.rs:440–445`PlanRoot分支continue；真实caller `ready_stock_stream.rs:61/64`调用ready_batch_without_waiting_for_roots。所属 `stock_stream/root_ready_tests.rs:112` 在`:113`分别覆盖1/2 workers。 |
| 27行第5项：local admission fast path、receipt推进与首错 | `local_admission.rs:68`短批先observe receipt，`:74`prepare，`:100`扫描receipt先于`:159`resource edges及quote依赖；外层失败仍丢私有tick，不新增局部retry原子承诺。 |
| 28行第6项：Cash/Shares资源边、Cancel→Place、实际股票gate、无无关总序 | `local_admission.rs:110` Cash(owner)、`:119` Shares(owner,stock)、`:124`普通Cancel无资源lane；`:159`只排冲突lane，`:198`显式quote依赖。stock gate仅局部受理，不恢复npc→player→plan全局排序。既有gate可能的性能边界不由重构完成核销。 |
| 29行第7项：金额分/股数/T+1/资源/时段/DTO不变 | `session_execution_transaction.rs:59`仍读取setup.t1_enabled；真实机构成交测试 `session_execution_transaction_tests.rs:151`核锁定100股。DTO仍保持字段读取，TradingPlan/Account内部走getter；不以统一getter要求改SaveSlot wire形状。 |
| 30行第8项：attention O(1)队头 | `npc_tick_preparation.rs:121`读取唯一attention_scheduler.next_scheduled_tick；`session/attention.rs:272`仅BinaryHeap.peek，未扫描全体NPC。 |
| 31行第9项：participant仅替换belief，其他三成员保留；missing typed error在安装前 | `session_execution_transaction.rs:90`remove既有participant，`:96`只写belief_mut，`:97`回原key；`institutional_experience_projection.rs:47/51`预先检查missing participant，`:71/74`构造patch时再次显式拒绝。`:95`的expect对应已准备完成的不变量，不是恢复无校验默认对象。 |
| 33–35行：owner范围、finish/Settlement/P9不移入stream | StockStreamCoordinator只持本tick任务状态；`continuous_tick_transaction.rs:234`流finish后`:250/256`再finalize；`candidate_commit.rs:98`唯一commit。没有常驻权威actor或重复结算路径。 |
| 37–51行：S1机构非空belief_patch直接回归缺口及闭环 | **已修复。** 当前 `session_execution_transaction_tests.rs:89` 真实机构Fill经P3/P4和SessionExecutionTransaction，`:138`核两腿收据，`:154–161`核非空information/watchlist/price_memory及另一participant四成员，`:163`之后核另一account事实和策略。不再列测试缺口。 |
| 53–60行：编译02 TradingPlan私有字段遗漏 | **已修复。** `continuous_tick_transaction_tests.rs:511/512/524/560/562` 与 `pre_open_transaction_tests.rs:405` 使用filled_qty/status/active_child_order_id getter；`plans/state.rs:214/230/258`仅读取原事实。SaveSlot.plans.plan同样返回TradingPlan，已适配，无存档schema变更。 |
| 62–68行：编译03 Position事实比较、最终机构测试指纹 | **已修复。** 当前测试`:165`按StockCode完整集合比较qty/t1_locked/invested_cents/recovered_cents；`account.rs:609` Position只包含此四项权威事实，没有长度比较或为测试扩生产derive。当前文件SHA-256为 `06b675be990c1e0e2eb5661a942022d769f5ba03264618122077b94f7a597d84`，与最终记录一致。 |
| 70–77行：最终35文件SHA绑定、具名静态复核、不代签测试 | 当前35/35 hash相符；这仅确认版本绑定，不声称本轮重新读35文件完整diff或运行测试。原review的作者独立性/完整diff覆盖按历史具名记录保留，不由本轮摘要改写。 |

## state-callers0.md 条款矩阵

| 原文位置/承诺 | 当前实现与限定 |
| --- | --- |
| 3–12行：Session state、Account getter/fixture/restore_strategy、Parent/TradingPlan、Candle、attention、participant参数 | `account_validation_context.rs:39`走session.state；`npc_state_projection.rs:196/206`take_strategy/restore_strategy；`account.rs:152`生产restore与`:157/167`cfg(test)fixture明确分开。`session/execution.rs:52`ParentOrderPlan::from_facts是内部fixture事实入口；`pre_open_transaction_tests.rs:61/62`通过candle_book.histories/active读取；`stock_execution_transaction.rs:188/191`调用prepare_settlement_transaction_with_beliefs传唯一participant集合。主干已有。 |
| 14行：retail_projection_persistence_tests转交projections | 迁移仍在当前源码：该文件`:151`走state.setup，`:212/219` Account/Position getter，`:210`恢复receipt cursor。转交属主不构成漏改caller；不按此组重复实施。 |
| 16–32行：15实际修改文件清单 | 覆盖访问迁移与最终review manifest版本；没有提出15个以外每个分配文件必须修改的目标。 |
| 34–40行：保留交易/失败边界、格式/扫描、未编译测试 | 历史记录准确限制验证范围。后续summary与final-validation已有集中编译/短测记录，不把本实施者当时未执行改写成测试已运行，也不据此推断当前从未验证。 |
| 42–44行：TradingPlan后补getter | 最终当前两测试文件已使用getter，同review编译02条目闭环。 |

## state-callers1.md 条款矩阵

| 原文位置/承诺 | 当前实现与限定 |
| --- | --- |
| 3–14行：state/Account/Position/attention/Parent/participant/TickShadow迁移 | `pipeline/shadow.rs:54/71/74`链式Option进入state再读strategy getter；`session_execution_transaction.rs:90–97`只改belief；attention唯一owner见上。剩余ResVec/SelfView/EnvelopeReceipt DTO字段不要求getter，不能把`.cash/.qty`命中全部登记为漏迁。 |
| 16–20行：17实际迁移文件、格式、未cargo与待统一验证 | 属历史实施阶段的验证限制，不是17个生产功能未完成。正式当前验证由后续final-validation负责；本轮没有重复执行。 |
| 22–61行：36文件行数/哈希清单 | 分配文件版本表不是36个全部都需变更的实施清单。最终review manifest的35实际变更文件全部hash相符；该文中较早版本不与最终作者签名混合成“新版本测试通过”。 |
| 63–69行：非空belief_patch测试、具体filter、待独立审核/执行 | 当前精确test symbol存在；后续review的S1/编译03/最终SHA签名覆盖，final-validation记录实际case结果，见下文。 |
| 71行：249行/605489测试中间指纹 | **历史中间版本已由后续明确修订替代。** 当前266行/06b675与review最终指纹一致。该249行版本不能拿来复用最终测试证据，也不能因字节不同登记产品遗漏；Position事实比较修复解释了增量。 |

## 后续验证证据与新候选反证

- `agents/oop-refactor-implementation/validation/final-validation.json:2064` 记录S1精确case `institutional_fill_updates_belief_and_preserves_participant_members_and_other_account`，passed=true、exit_code=0、timed_out=false、seconds=0.1451；绑定source_sha256为当前06b675。此为读取既有证据，本轮未运行该case。
- 后续 `agents/oop-refactor-implementation/summary.md` 记录四package编译/check与278唯一Rust短case，并明确未做复杂回归/E2E/长期矩阵。这能关闭“统一验证尚未开始”的过时解读，不能证明本轮/全部交易路径/性能验收通过。root_ready存在测试，不在本轮因查到源码就宣称已重跑。
- **候选“重建participant丢其他三成员”不成立：** 生产写回只替换既有participant.belief，非空测试检查三成员和另一账户，最终hash与运行记录相符。另一 `account_settlement.rs:120–125` 的prepared patch也采用相同保留原participant的写法，不存在第二条清空路径。
- **候选“两个channel乱序漏股票”不成立：** Stock通知仅在payload成功发送后出现，每次Stock通知消费任意一个已到payload；PlanRoot分支不消费payload。要求两channel同序会引入当前未要求的全局顺序。
- **候选“raw私有字段/SaveSlot getter遗漏”不成立：** 当前迁移主干和两个补修测试均适配新API；DTO公开字段保留是原兼容要求，不是漏实现。未用grep替代编译运行证据。

本轮未确认新增生产代码遗漏；S1、编译02与编译03均有当前源码闭环及最终SHA绑定。原总账G16/G38/G39及其他正式功能缺口不因OOP caller迁移完成而核销，也不要求恢复旧固定来源排序或退役语料工具。
