# Task 14–16 EOF 历史复核：当前调用链与 G36

## 覆盖与方法

已从首行到 EOF 连续读取以下三个完整文件，共 **521 行**；Task 14 原始 `REJECT`、其修复复验及最终 `APPROVE` 均在内：

| 文件 | 行数 | 全文结构 |
|---|---:|---|
| `.omo/evidence/company-information-npc-intentions/task-14-review.md` | 230 | 初始复核与原 F 区/REJECT（1–190）；`ddf55e1`、`404ae9b` 修复逐项复验及最终 APPROVE（191–230） |
| `.omo/evidence/company-information-npc-intentions/task-15-review.md` | 170 | 三问、K4 探针、登记核查、D1–D7、历史隔离复跑、观察及 APPROVE（全文） |
| `.omo/evidence/company-information-npc-intentions/task-16-review.md` | 121 | 三问、验收/失败矩阵、登记检查、worker 对账、隔离复跑、观察及 APPROVE（全文） |

对照计划 Task 14–16（`docs/superpowers/plans/2026-09-10-company-information-npc-intentions.md:387–412`）以及当前 `engine` 生产源的 owner→caller→consumer。历史测试计数只作为当时证据，不据此宣称当前测试通过；本轮未运行测试，也未改产品代码或 Git 状态。

## 逐章对照当前代码

| 历史章节/锚点 | 当前生产实现与调用链 | 当前判定 |
|---|---|---|
| Task 14 §二 Q1：经营事件、行业适用面、事件目录（42–94） | `sample_company_shock` 只收 RNG、日期、参数，六类公司冲击均匀采样（`packages/engine/src/company/events.rs:209–236`）；`OperatingDayRun::sample_and_activate_shocks` 将样本直接加到对应公司的 `economy`（`packages/engine/src/company/operations/day.rs:135–145`）。到期/事件派发后按公司 `FlowParams` 进入各行业经营流（`:150–205`）。 | 经营循环与字段消费仍存在；行业适用性没有落实到随机抽样/激活层。不能由“事件进入 active”推断“该行业消费了事件”。 |
| Task 14 §二 Q1：期限、持久队列；§四 F-O2（158） | `FlowParams::validate_durations` 对四行业期限字段逐项拒绝 `<1`（`operations/config.rs:156–186`）；`CompanyOperations::build` 在每个公司 `IndustryPairView::at_build_guard` 前执行校验（`operations/core.rs:175–205`），live/history 共用 builder。Due dispatcher 和 scheduler 保持类型化错误出口。 | 原 F-O2 已由修复解决；不把历史 `DueSkipped` 配置脚枪重新列为现行遗漏。当前适用期限字段清单以校验实现为准，未发现该历史 0 日期限遗漏。 |
| Task 14 §三、§四 F1/F2/F-O4；§末 Re-verification（191–230） | 复核全文记载 F1 游戏假设已登记、F2 clippy 修复、F-O4 注释修正；当前文件/fixture 正式文档位于 `docs/company-accounting.md` 与 `packages/engine/tests/fixtures/company-model/policy-sources.json`。当前 `bank.rs` 注释已与 `newly CreditDeterioration || credit_risk_add_bp > 0` 条件一致（`operations/bank.rs:108–118`）。 | 历史 REJECT 的 F1/F2 已有明确修复复验；F-O2、F-O4 修复由当前代码再次确认。F-O3 mirrored 裁剪在当前生产路径存在（`session.rs` 日终经营后 `ops_wiring.prune_dispatched`）；不重开。F-O1 不属于已修复项，转见 G36。 |
| Task 14 §四 F-O1（153）；Task 15 §三问 Q1、K4 探针 F-O1（约 38、93） | Task 15 历史裁定以 `economy().active()` 且 `starts_on == settled` 认为惰性事件不会公告。当前公告 caller 实际是 `DisclosureDispatch::run_day_end`（`session/disclosures.rs:107–126`）：只按日期筛选，再将每个 active `shock` 原样交给 `AnnouncedEvent::from_active` 和 `PublicLibrary`。而 Task 14 抽样无行业参数，全部六类随机事件都会激活（见上）。`events.rs:12–20` 自己限定中断仅工商/地产、减值迹象仅工商；银行/保险不适用。 | **旧核销理由被当前调用链反证，G36 仍成立。** active 代表有效窗口内，不代表适用于该行业；`starts_on` 是时间过滤，不是行业/冲击消费过滤。旧 Task 15 “惰性事件无公告路径”不成立。公告是真实公共记录，不能以日报告被 session 丢弃（其结果未用作公告筛选）核销。 |
| Task 15 §一 Q1 / §K4：排期、定期披露、定稿及更正（21–57、78–95） | 当前 18:00 日终 caller 顺序仍是经营→`close_accounting_periods`→`DisclosureDispatch::run_day_end`（`session.rs:2105–2118`）；排期报告从登记簿和公司单体 books 构建，成功后事件 ID 才追加（`session/disclosures.rs:129+`、`session.rs:2130–2176`）。 | Task 15 对不可变公开库、排期及时序的 APPROVE 不覆盖公告适用性缺口。当前日终定期报告仍是独立闭环；集团 G28、行业会话 G36 不能混为 Task 15 模块缺陷。 |
| Task 15 §偏差 D2（123）与当前封账 caller | 当前 Session 已在披露前接入月/年封账（`session.rs:2110`；`close_accounting_periods` 与 `close_month`/`close_year` 后续定义），故原 D2 “封账结构性不可达”的历史条件已变。 | 不把“完全没有月/年封账”重开为现行缺陷；非工商 books mut / 自定义四行业会话能力仍是 G36 已登记范围。 |
| Task 16 §一、§二：候选→本人获知→只读上下文（15–65） | 当前生产获知路径在 `InstitutionDecisionRoot::observe_personal`：本人候选代码映射发行人、`discovery_candidates` 列公开 ID、过滤本人已持有记录、`record_acquisition` 记录，再构造 `NpcObservationContext`（`session/decision_chain/roots.rs:345–405`）。公共曝光仅为候选；`acquisition.rs:141+` 仍检查 owner、公开 ID 与 `observed_at`，读取侧仍要求 acquired。 | 机构 root 的本人获知链实际存在且符合 Task 16 的“候选不等于阅读”。历史 Task 16 对模块边界/纯接口的 APPROVE 有效，但不能推导所有 NPC 类型和所有报告种类均已接入预测消费。根只把新年报收进 `new_annual_reports`（roots.rs:366–396）；季度/半年报及公告虽获知，是否更新判断是后续消费者契约（沿 G09/Q11 现有台账，不在本次新编号）。 |
| Task 16 §三、§六：已登记决定及观察（69–121） | 实现仍保留幂等获知与读取时库守卫；常规调用点除机构 root 外，`PlanPersonalState` 内出现的是事务 shadow 状态的装入/回滚测试辅助路径（`decision_chain/personal_state.rs:165`），其他搜索命中是集成测试。 | 不把测试、恢复/事务投影路径算成额外生产获知 caller；没有发现绕过本人状态直接读取全部公开材料的新路径。 |

## G36 公告过滤复核与新增边界

**可复核的公告链**：`sample_company_shock`（无 `CompanyKind`）→ `OperatingDayRun` 对样本调用 `economy.activate` → 日终 `DisclosureDispatch` 遍历 `active()`，以 `starts_on` 为唯一逐事件过滤 → `AnnouncedEvent::from_active` 写入全局 `PublicLibrary` → `GameSession::record_civil_day_events` 发 `CompanyDisclosurePublished` 事件（`session.rs:2130–2148`）。不需要猜测随机结果即可确认路径；本轮未声称默认游戏已实际抽到不适用事件。

1. **重证历史误核销（G36）**：银行/保险可取得 `ProductionInterruption` 与 `AssetImpairmentSignal`，而目录明示前者仅工商/地产、后者仅工商（`events.rs:12–20`）。公告器不识别消费行业或适用类别。因此 Task 15 复核把 active 当作已适用事实过滤的结论错误，Task 14 F-O1 观察仍未关闭。
2. **额外的同类漏滤候选，仍归 G36**：银行 flow 唯一读 `aggregates.credit_risk_add_bp`（`operations/bank.rs:111–118`）；公司 `CompanyDemandShift`、`ContractWon`、`ContractCancelled` 会被 `EconomyAggregates::aggregates` 合并进需求（`operations/state.rs:57–65`），但银行 flow 不读需求（`rg` 对 `operations/bank.rs` 的 `aggregates.` 只有信用字段）。然而这三类仍会按上面的公告链对银行生成公共公告。源头文档也明示银行存贷不读需求（`events.rs:12–16`）。这是同一“无适用性过滤”断链的额外可证样本，不另建 G 编号；处理前需明确它们对银行是否仅无经营效果、还是根本不是银行可公告事实。
3. **边界不扩大**：保险与地产的 `CreditDeterioration` 已登记为“记录风险事件、无自动重估面”（`events.rs:17–18`），它本身不能仅因缺少自动重估就判成错误公告；公告表示风险事件而不是实际信用损失。地产收到 `AssetImpairmentSignal` 是否应公告，也需区分“迹象事实”与“已发生减值”，不凭缺消费处理器就武断核销所有减值公告。重点应是为每个 `CompanyKind × ShockKind` 明确 eligibility 与公告语义，并使采样、经营效果、对外披露遵守同一矩阵。

## 旧发现处置与结论

- Task 14 原始 **REJECT** 只针对 F1 登记门禁与 F2 clippy 警告；文末记录两个修复提交及相应复验结果。当前复核以该完整复验和现行代码核对为据，判为已解决，不重复拒绝。F-O2/F-O4 已复验并在当前源上确认；F-O3 后续裁剪已有生产 caller。
- Task 15 原 `APPROVE` 的普通定期披露、公开库不可变性、排期/时序结论不被本复核推翻；其中 F-O1“惰性事件无公告路径”的单项理由经现行源反证，不能作为已核销证据。
- Task 16 原 `APPROVE` 的模块层隔离结论仍成立；当前机构 caller 真正接入候选与本人获知。其观察项及报告消费限制不因本次而扩号，沿 G07/G09/Q11 既有范围。
- 本次结论：历史实现与当时门禁的复验记录可信地解释其当时结论；当前仍有 G36 生产范围问题，且银行需求类事件提供额外的同类漏滤证据。只更新 `agents/implementation-audit/exhaustive-review/luna24.md`，不改产品或登记新编号；应由总审计维护者决定将新增样本并入 G36 说明。
