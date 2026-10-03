# Task 27/28 全文独立复核

复核基线：产品提交 `08e4fc7`，当前 worktree 为其 merge 后提交 `a7c7ce3`。本复核未运行测试；仅检查提交中的源码与以下三份材料的完整内容。

## 全文读取记录

| 文件 | 全文行数 | 逐章范围 |
|---|---:|---|
| `.omo/evidence/company-information-npc-intentions/task-27-manual-continuation.md` | 25 | 标题/日期/真实数据面/1–5续行步骤/observed command与结果/容量回执/修复回执，1–25 全部 |
| `.omo/evidence/company-information-npc-intentions/task-27-review.md` | 54 | 标题/日期/结论/Final Closure/Retained Independent Behavior Evidence表/语义与K7断言/Residual Risks，1–54 全部 |
| `.omo/evidence/company-information-npc-intentions/task-28-review.md` | 89 | 标题/结论/Acceptance Coverage Table/五项Rejections/证据范围/复现验证表/Cleanup Receipt，1–89 全部 |

共 168 行。以下按原文各章节和 R1–R5 逐项核对，不把旧结论直接沿用到当前实现。

## Task 27 两份回执

| 原文范围 | 当前源码核对 | 复核结论 |
|---|---|---|
| continuation receipt 1–5、真实 26-NPC/5-stock 数据面 | `tests/save_contract/main.rs:38-84` 的 `contract_setup` 是 1 stock、5 NPC；`:87-103` 的 `continuity_setup` 是 1 stock、3 NPC、每日 1 tick。`:263-300` 的 `restore_is_byte_continuous_with_uninterrupted_run` 只在首次存档前走 1 tick+日结，恢复后比较两个交易 tick、一个日结和次日首 tick。两日完整决策链和保存所有个人状态另由 `:115-147`、`:212-260` 覆盖。 | 当前源码没有回执所描述的“真实 5-stock、26-NPC，存档恢复后继续三整日”的同一测试。`git blame` 显示续行测试在 `f222417`（2026-09-23）把 tick/day 缩为 1；回执日期为 2026-09-12，故历史结果不据此判为伪造，但它不能替代对当前实现的同规模复核。当前 K7 最小模型保存/恢复证据仍在，覆盖规模与持续长度已经不同。 |
| Task-27 review Final Closure，1–14 | 当前 merge 基线下的材料正文和源码可见，但本轮没有重跑、不核对该回执所称生成绑定清理后的工作树状态。 | 这是当时的收尾证据，不等同当前提交的生成路径状态；不发现可以据此复活已关闭 P1 的新产品缺陷。 |
| Retained Independent Behavior Evidence 表，16–25；Residual Risks，48–54 | `save_contract/main.rs:187-260` 仍以真实决策链生成非空 `belief_books`、已获取 publications、price memories，并检查 save→decode→restore 后权威 JSON 字节一致；`:263-300` 仍有跨恢复事件/存档比较。容量路径原文结果本轮未执行。 | 与当前较小fixture的职责相符；表中旧命令结果属于历史记录，本轮不声称它们在 `08e4fc7` 上重新通过。`task-27-review.md` 认可的“26-NPC、恢复后三天”覆盖不能从现行 `restore_is...` 单测名推定仍在。 |
| K7、旧迁移、交易语义结论，26–47 | `session.rs:2658-2684` 将公司、披露、计划、个人信息、信念、关注与价格记忆状态写入 save；`:2716-2721` 先 `validate_save_slot` 再构造候选 `GameSession`。`save_contract/main.rs:212-260` 覆盖现格式精确恢复。 | 存档内容与字节相同性主张有现行代码依据。task27材料未提供本轮A股制度变化证据；审查对象属于持久化续行，不据此改变或概括沪深规则。 |

## Task 28 覆盖矩阵及旧 R1–R5

| 原文范围/旧判断 | 当前源码和调用路径 | 当前结论 |
|---|---|---|
| Coverage 18、R3 47–51：闭市日只检查日期和pending，未证明年报/披露 | `tests/company_scenarios/lifecycle.rs:50-154` 增加真实年末 `GameSession`；`69-99` 检查结账版本增长，`:101-113` 通过公开查询找年度报告，`:115-144` 跨 civil days 推进、断言 closed day 不增加 tick/day、disclosure cursor 与报告期间/余额。 | 旧事实已修复，不复活 R3 的原始指控。但用例在 `:51` 标为 `#[ignore]`，当前普通目标默认不会运行此路径；应把它记为长验收未在当前任务证据中执行，不能把源码存在等同执行通过。 |
| Coverage 20、R1：同公开材料不同先验仍是纯 `revise_forecast` 测试 | 旧 `participant.rs` 已移除。`company_scenarios/controlled.rs:8-90` 从实际历史报告构造已取得的个人先验并 restore；`:91-118` 通过 `GameSession::query_public_reports` 控制公开时点；`:143-181` 经正常 tick/日结/队列消费，验证两账户都取得相同新 report 且估值不同。 | 旧的“绕过 GameSession”结论已失效。历史 prior 用 `NpcInformationState`/`BeliefBook` 领域接口装入受控 save，当前新 report 获取及信念修正确实经真实 session chain。此证据不主张所有历史形成步骤都来自自然随机路径。 |
| Coverage 21、R1：同P&L不同经历直接调用allocation函数 | `controlled_experience.rs:3-87` 创建个人 dated 失败经历并恢复两个真实 session；`:89-107` 各执行一次 session tick，比较现金相同、`last_retail_decisions` 不同。 | 旧“没有session消费”结论已修复。受控历史通过 save fixture 注入是差异变量；测试证明生产决策 tick 消费该经历，不证明机构 allocation 全链的所有边界。 |
| Coverage 22、R1：cheap-but-withdraw直接构造QuoteAction | 当前 `session/decision_chain.rs:2354-2396` 的 session 测试检查 stale buy 的真实 `OrderCanceled`、无新 `OrderAccepted`、计划终止及无仓位变化；`:2398-2405` 覆盖中性和负弱信号。 | “撤单不经session路由”的旧结论已修复。该用例是 recovery review 的中性/弱负撤买，并不完全等同 ADR 所述“估值仍便宜且撤买”的特定联合信号；将此限制作场景覆盖债，不据此认定生产撤单错误。 |
| Coverage 23、R1：跨股票卖单不能为买单提供现金 | `decision_chain.rs:484-500` 的生产 resource snapshot 仅取本人现金并扣本人冻结现金；`pipeline/decision_resources_tests.rs:209-255` 有混合订单与 pending event 的资源测试。 | 没找到公司 scenario 中“跨股真实卖单未成交→买单不获资金”的完整闭环。生产输入及局部保护有证据，旧纯函数测试缺口不能单独证明生产错误；列为验收尚待场景证明。 |
| Coverage 24–25、R4：真实 partial fill 和 restore 不相连 | `session/pipeline/continuous_tick_transaction_tests.rs:520-596` 的 `live_plan_partial_fill_survives_restore_and_second_real_tick_fill` 保存真实部分成交后的 session，restore 同一 child，执行第二个真实 tick 成交，再核对 filled qty、child remainder、reserve、持仓与两条路径快照。 | 旧“只有分离场景、没有真实partial恢复续行”的判断已不成立。现有测试没有逐字节比较所有事件和完整最终 save；它至少证明同一真实订单部分成交、存档恢复、继续成交及主要账户对账。不要要求或声称自由并发期间任意路径必须事件总序相同。 |
| Coverage 26、R2：T+1场景实际从零库存起卖出 | 原错误 `constraints.rs` 路径已不在当前 `company_scenarios`。T+1生产测试可见于 `session/pipeline/continuous_trade_acceptance_tests.rs:341-415`（真实买入成交后核对 `t1_locked`/sellable qty），以及 `tests/session.rs:3599` 起的账户日界；其他 session validator 也检查 `InsufficientShares`。 | 旧“测试只是零库存拒绝”的具体指控针对已删除用例，不复活它。但当前 task-28 scenario target没有新的“同一普通session买入真实成交→立即卖出被T+1拒绝”场景。产品T+1路径有分层证据，此处只记任务场景验收缺口，不报产品语义回归。 |
| Coverage 27、R4：普通 restore 场景没有订单链 | `company_scenarios/restore.rs:4-43` 仍是普通真实同seed save/decode/restore事件与日末save连续性场景，无显式部分成交断言；连接partial的专门session测试见上一行。 | 普通 restore 用例自身范围仍有限，不能单独声称 partial-fill覆盖；但R4已由另一真实 session 生产测试补上，不能因文件仍分离而重开原R4。 |
| Coverage 28、R5：feature是空列表，两个运行只比测试名 | `Cargo.toml:13-32` 的 feature虽无额外依赖，但 engine源码有大量 `#[cfg(feature="simulation-diagnostics")]` collector/trace路径；`tests/diagnostic_absence.rs:72-94` 覆盖无feature时正常事件和save字节；`tests/diagnostic_parity.rs:56-83` 覆盖带feature时trace查询只读/有界；`company_scenarios/controlled.rs:184-211` 比较同一build中读取diagnostic API的双session权威事件和save字节。Web Worker在 `host/wasm-worker.ts:106-125` 只在DEV按flag选择diagnostic WASM，release固定常规包。 | R5关于“空feature/只有test list”已失效；不复活。独立分层测试与当前 task28 测试各有明确边界。task28自身的双session断言是在同一个编译配置内运行，不能当作跨feature binary的直接byte-parity结果；cross-config parity若是验收条件，应由两配置驱动或另一个专属验收证据明确证明。 |
| Coverage 17、R1：真实GameSession/closed-day基础链 | `company_scenarios/lifecycle.rs:7-48` 以真实session断言 closed civil day 不推进 tick、day、RNG，未把此例夸大为撮合链。 | 旧覆盖表的“Partial”对该单一场景本身仍准确；它与同target其他场景合看。 |

## 证据与风险边界

- `task-28-review.md` 中 `participant.rs`、`constraints.rs`、`matching.rs` 等路径是旧版结构。本次检查确认目录现拆为 `controlled.rs`、`controlled_experience.rs`、`lifecycle.rs`、`restore.rs`；不能沿用旧路径上的旧 REJECT 作为现行缺陷事实。
- 旧 R3/R4/R5 的核心反证已在当前源码找到。R3虽然实现了跨年公开报告链，仍是 ignored long validation；R4由真实部分成交恢复续行测试迁移覆盖；R5的诊断feature已实际条件编译且有无feature测试。
- task28计划 `company-information-npc-intentions.md:530-537` 仍写有跨年/报告/休市、T+1负例、同损益经历、cheap-but-withdraw、跨股卖未成交不买及“diagnostics开关”验收。当前scenario源码没有逐项兑现全部列举情景：明确未找到同session T+1负例和跨股票卖出/买入场景；cheap-but-withdraw联合信号未由已读session用例精确证明；year boundary 测试被ignore。它们是验收证据债，不据测试缺少推断产品行为错误。
- `task-28-review.md` 的旧运行计数/两次 `--list` 结果及 task27 收据的历史运行结果，都不是本次对 `08e4fc7` 重放的测试结果。本轮没有运行命令。
- 领域范围为 save/restore、信息和诊断审查；没有发现当前材料或本次查看的变更要求修改交易制度。本结论不代替官方交易所规则来源审查，也不主张本轮重新验证沪深所有规则。

## 新候选

| ID | 级别 | 候选问题 | 反证与处置 |
|---|---|---|---|
| L28-01 | 验收证据债 | 当前 task28场景仍缺同session真实买入→T+1即时卖出拒绝及跨股票挂售未成交→买单资金隔离的端到端断言；年度闭市披露用例默认 ignored。 | 现行低层T+1、会计披露与资源约束有相应源码/测试；未发现产品路径缺陷。保留为task验收证据债，不重建已删除旧测试，不宣告task28全验收通过。 |
| L28-02 | 历史回执范围 | task27 2026-09-12 continuation receipt写明的26-NPC/5-stock三日恢复续跑不能由当前 `restore_is_byte_continuous_with_uninterrupted_run`复现：当前fixture和跨恢复长度已在2026-09-23变小。 | 历史记录日期早于源码缩减，可能如实记录当时运行。当前新格式两日真实状态往返与较小同seed续行仍在源码中；若要求26-NPC压力续行作为现行封门，应新增短/受控复验或明确接纳历史证据边界。 |
| L28-03 | parity声明边界 | `company_scenarios` 的 `reading_diagnostics...` 证明同一build中查询diagnostics不改事件和save，不直接证明 feature 开/关两份编译物的完整byte-parity。 | `diagnostic_absence` 与 `diagnostic_parity` 分别覆盖两侧只读/缺失边界，当前不支持旧 R5“feature空/无collector”的说法。归属跨feature完整Parity的正式任务/脚本需单独确认，不报告为当前产品差异。 |

**复核结论：** 不接受 task-28 原 REJECT 中仍引用旧文件的R1–R5作为现行产品缺陷；R3/R4/R5主张已修复或迁移，R1受控先验和经历场景也已接入session。保留上述三个边界，不把证据缺口扩大为产品错误。task27历史回执可信度不作否定，但当前代码覆盖规模与回执描述有时间差，应明确区分历史执行与当前提交可见测试。
