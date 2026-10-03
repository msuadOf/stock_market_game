# Luna46：公司信息三份历史工作文档全文复核

## 范围与方法

- 目标树：产品提交 `08e4fc7`（当前工作树为其 merge commit `a7c7ce3`）；本文仅新增工作记录，不改产品代码、正式文档或 Git 状态。
- 全文 EOF：`2026-09-11-company-information-handoff.md` 85 行，`2026-09-13-company-information-issues.md` 1391 行，`2026-09-13-company-information-learnings.md` 1499 行，共 2975 行。长文按连续行段读取，覆盖至各自 EOF；另读根 `AGENTS.md` 与 `docs/principles.md`。
- 复核方式：以行号矩阵覆盖每章主题；对历史阻断逐项查后续结论及当前调用者/源码；只把可由当前树证实的差异列为发现。乱码段按任务要求不推测还原。

## 结论摘要

1. 这三份文件是实施过程记录，不是可靠的当前状态入口。handoff 的 26/46、任务 27 WIP、未完成后续批次等均已被当前计划状态取代：`.omo/plans/company-information-npc-intentions.md` 中 1–36、39 标 `[x]`，37/38/40–42 为 `[~]`。不得用 handoff §1/§2 状态汇报当前进度。
2. 重要阻断大多有后续修复证据：Task 12 重复往来配对、Task 13 更正跨期重复计入及 `unwrap_or(zero)`、Task 25 登记 REJECT、Task 31 服务端安全与恢复、Task 32 共享事件/查询、Task 34 Worker DTO 生产边界、Task 35 特权适配器、Task 36 因果时钟均已有后续落实；下表逐项指出后续记录与现行代码锚点。
3. **有效的新发现：Task 39 的“现行 8 MiB Server body gate”描述已过期。** 当前 `apps/server/src/routes.rs:41` 的限制是 `engine::MAX_SAVE_DECODE_BYTES + 1 MiB`，`packages/engine/src/session/persistence.rs:944` 是 512 MiB decode 上限；ADR-0019 与 `docs/implementation-gaps.md:174-176` 也已更正旧说法。原记录的 29/69/133 MB 样本本身仍是历史数据，但“全部超过现行 8 MiB，故当前不可远程 load”推论不成立。新限制也不意味着无限容量：`agents/main-release-validation/summary.md` 记 10 万完整日存档 591,344,527 bytes 超过 536,870,912 bytes Engine 解码上限；HTTP 路由还执行嵌套深度检查。当前可表述为旧 8 MiB blocker 已被限额演进取代、最大规模样本仍越界；不要把早期样本继续写成现行 8 MiB 失败，也不要反向声称远程大档均已验收。
4. **历史独立复核拒绝项不可从旧段落单独推断仍未修。** 后续 append-only 记录已给出修复、复验或状态边界；历史 REJECT 是审计证据，不应删改，但现行总账应明确 supersede 关系。

## 全章主题矩阵

### Handoff：85 行

| 行 | 原文主题 | 后续/当前证据与裁决 |
|---|---|---|
| 1–25 | 2026-09-11 当时进度 26/46、1–26 复核状态、旧分支/基线 | 时间快照；当前计划把 1–36、39 勾完，37/38/40–42 尚为部分态。仅可作历史交接材料。 |
| 27–38 | Task 27 持久化 WIP、未验证 save_contract、证据重写待办 | 后续 Task 27 review follow-up（issues 1240–1244、learnings 1275–1285）称独立复核 APPROVE、绑定清理完成；handoff 的 WIP 指引过时。计划 27 当前 `[x]`。 |
| 40–51 | Task 27–42 依赖图与任务顺序 | 与当前计划完成情况不同；现行节点见 plan 521–664。依赖图可作为历史安排，不可作为当前剩余任务列表。 |
| 53–59 | Node/pnpm/Rust 环境、启动步骤及任务并行偏好 | 版本/环境为当时条件，且用户操作流程属于历史 OpenCode 语境；与现行 worktree/build 环境不等价。任务完成状态应看当前 plan 与发布记录。 |
| 61–71 | K7/过渡态/RNG/save/工具链/clippy/证据目录/engine_error_events | 其中任务 27 持久化过渡已结项；旧 lint 清单须由后续 lint 结论更新。不得照抄“仍需重放经营”或旧锚点。 |
| 73–79 | Task 27/29 探索摘要、256 公司配额撤销说明 | 行 75 明确 ADR-0019 撤销旧数量门槛；其余是 Task 27/29 前置设计和当时 DTO 约定，应以实现及后续复核为准。 |
| 81–85 | 旧 Git push 状态与证据提交状态 | 历史快照；当前用户任务明确限定只审，不执行 Git 外部操作。 |

### Issues：1391 行

| 行 | 原文主题 | 旧裁决复核 / 当前依据 |
|---|---|---|
| 7–12 | Todo 4 文档契约校验 | 后续 Todo 4 learnings 7–16 记录 checker、链接校验通过；仅证明当次文件集合。 |
| 14–25 | Task 39 8 MiB 容量、Task 35 privileged adapters 与 GUI blocker | Task 39 的 8 MiB 现行化结论过期（见摘要，新候选）。Task 35 后续 learnings 34–47 记依赖安装、MockRuntime actor 队列、Wry/Xvfb 启动和 stale-generation 修复；issues 24–25 的真实 curl/代码与 GUI native compile blocker 是当时不同验证层，不要把 GTK 缺失当现行产品缺陷。 |
| 27–63 | Task 30 WASM build/nullable/date 回归与 desktop 限制 | Task 30 learnings 49–78、1401–1425 记录 own-null、period-end 在共享 `PublicReportSummary::from` 修正，并删除 WASM 特有日期转换。问题条目保留的是历史尝试约束；适用结论为共享层修复。 |
| 65–76 | Task 32 原始 Tauri IPC、事件契约及 GTK 阻断 | 被 issues 78–82、learnings 80–96、1332–1349、1351–1362 取代：Task 29 增加 engine 共用事件/查询，Tauri actor 复用 `end_civil_day`；Native GUI 运行限制后来有独立环境解决证据。无桌面专属协议是正确约束。 |
| 78–89 | Task 29 engine event 完成、Task 33 web/cache review 修复 | Learnings 100–119 和 1351–1362 说明 strict shared DTO parsing、metadata cache accepted-delivery 更新、全局 sequence 的 engine 事件。旧阻断已收敛。 |
| 91–103 | Todo 2 Node/Clippy blocker；Task 29 host event 下游提醒 | Task 31 后续 evidence 修正 actor 批量转发；`docs/implementation-gaps.md`/Todo 2 与当前工具链记录应优先。handoff 的“不在 PATH”不能泛化到新 runtime。 |
| 107–130 | W1-1 baseline、LOC 张力、engine_error_events、pnpm 和独立 review 欠账 | 历史补偿性 review 未替代独立门禁；后续项目已有不同审查记录。engine_error_events 观测项语义继续成立，但旧机器成本和基线值不能当当前发布样本。 |
| 133–188 | W1-3/2 拆分、政策取证、官方源阻断及 review 欠账 | 224–240 后续把多份来源从 blocked 改为 verified，并明确 2006 批部分不可得；需保留“依据适用日/范围”区分。Task 8–15 简化登记后来有 task-specific docs+fixture review 补记。乱码内容不解释。 |
| 190–222 | Task 21 plan 状态机、守卫不可达 REJECT | issues 216–222 明记 `Expired` 与迟到日终修复，learnings 293–312 有可达性原因与测试；当前状态机在 `packages/engine/src/plans/`，旧 REJECT 已修。 |
| 224–240 | 官方材料第二轮取证 | 记录了首轮误判更正、CAS25 解阻与仍不可得项；属于领域依据历史。不要将其等同于当前各行业生产支持，因为简化/blocked 是逐任务登记。 |
| 243–267 | W1-4 日历章节（严重 mojibake） | 按要求不猜测乱码，不从可读零散字符逆向补造。后续可读日历知识见 learnings 315–336（也有 mojibake）及仓库现行日历/政策源；本审计不把乱码当证据。 |
| 268–304 | Accounting、civil clock、Task 5 review 与跨任务约定 | 旧 account/时钟 API 细节可按 learnings 337–429 对照。Task 5 的 N3/N4 后由 Task 27/28 负责；不可把当时未实现的事件当现行缺陷。 |
| 305–369 | Task 7、17、8：公司实体、profile、工业会计与简化 | 学习记录 430–549 对应模块与约束；A 股语义简化需查当前 `docs/company-accounting.md`/policy fixture，不应仅凭 notepad 注释判断正式登记是否已完成。行业任务 8–11 后续都有单独 review。 |
| 370–408 | Task 8/17 换机交接（乱码、旧 HEAD/证据状态） | 大段乱码不复原；只保留可读提交号作为历史线索，不把旧 checklist 判定为当前状态。 |
| 410–420 | Task 8 review 的零费/空税/零天计息等边界建议 | 是候选覆盖债，不是断言当前必缺；后来行业 gold/failure suites 与全验收需判断具体分支。无证据不可声称这些每一项皆已新增专测。 |
| 422–503 | Task 19/9、ts_rs 绑定积压及银行 review 次要观察 | Task 29 后续统一绑定生成，issues 1242–1259 和 learnings 1299–1309 记 37 exports。Task 9 review 的本金回收/零 ECL/逾期续息/365d 缺口在历史处只登记建议；不能仅凭后来的总测试通过称每个被建议边界都逐一覆盖。 |
| 531–568 | Task 20 feedback 序列化、旧行为过渡态、共享树问题 | Task 26/27 已将 feedback 接入并收档；默认空序列化/旧锚是历史演进。按当前 Task 26/27 review 证据判定，不能把 Task 20 时未接线当现状。 |
| 569–647 | Task 11/10 行业会计、简化登记及 10 REJECT | Task 10 登记缺口曾为真实 blocker，issues 648–662 记录 docs+fixture-only 修复/复核结果。CAS17 仍是版本化游戏假设，不可作为现行 CAS 参数；insurance 简化必须对照正式文档。 |
| 664–779 | Task 12 合并范围、Task 14 经营、Task 12 REJECT 与发现 F2–F6 | Duplicate intercompany mirror reuse 已在当前 `packages/engine/src/accounting/consolidation/eliminate.rs:66` 通过已配对条目过滤（继续查看 68 起）；`tests/consolidation/failures/intercompany.rs:82` 有重复对拒绝测试。Task 12 的六条简化登记曾 REJECT，后来追加 docs/fixture 修复证据应追正式政策文档。Task14 采样行业污染、期限 0、注释不一致等均为当时 findings，后续 task 15/26/领域测试需核对；F3 测试体积是维护约定而非领域 blocker。 |
| 780–880 | Task 12 F3、Task 14 REJECT、Task 13 reports、Task 13 REJECT F1–F3 | Task 13 跨期更正问题有直接代码证据：`packages/engine/src/accounting/reports/window.rs:67` 接收 adjustment map，lifecycle gold `packages/engine/tests/industry_reports/lifecycle_gold.rs:352–370` 检查后续期间净利为零并调期初留存；`cash_flow.rs:73` 显式处理重述现金行。REJECT 不再是现行缺陷。政策登记修复仍应以 docs/fixture 为准。 |
| 881–981 | Task 15 report schedule/public library、Task 16 acquisition 及次要观察 | Task 16 APPPROVAL_HOUR 当前为 `information/publication.rs:27` 的常量并在 `prehistory.rs:158` 使用；报告排期、EarlyRead/获知分离可对应现代码/测试。O-1/O-2 是显式未覆盖组合候选，不应从总体套件通过推成专测已存在。 |
| 982–1010 | Task 25 独立复核 REJECT（发现权重未登记） | 行 1055–1062 记录错误基底否定及 `b2d88a9` 修复后的隔离复验 APPROVE；不得沿用 a885ac1。 |
| 1011–1048 | Task 18 beliefs/forecast/valuation 设计 | 对应 learnings 1087–1142。所有游戏假设参数曾待 Task 41 入册；当前应检查最新正式 policy sources，而非由此历史未登记直接推断。 |
| 1049–1062 | Git reset 事故、Task 25 re-verification | 乱码事故经恢复链与隔离复验闭合；原提交 `a885ac1` 不在最终链且无效。属流程历史，不是当前源码问题。 |
| 1064–1147 | Task 23/22/24 交易计划与大 A 边界 | 后续修复明确库存上限、100 股申报单位、排序/迟滞边界；Task24 的日中撤单、执行状态、restore 过渡契约继续由 Task 26/27 收口。交易数量语义仍须以当前 route/trading rules 为准。 |
| 1150–1238 | Task 26 全链集成、共同 V 删除、保存过渡态与 REJECT 修复轮 | `Task26` 的市场测试意外删除问题已修复：从旧提交恢复涨跌停/价格笼子语义并拆 `tests/market/`；当前 `packages/engine/tests/market/` 有 15 项。任务 27 接收过渡态后持久化，旧的重放经营/信念复位描述已经过期。 |
| 1240–1349 | Task 27–34 review/verification/DTO/UI 历史 | Task 27 APPROVE、28 初审 gap 已 remediated、29 types/events、30 bridge、31 privacy/auth/restore、32 Desktop、33 metadata、34 UI controlled-date 与浏览器修复均有 append-only 后证据。GUI capture/Watermark 等记录只按当时观察保留，不能由 code test 推断视觉已解决。 |
| 1351–1385 | Task 36 初始因事件字段受限而 BLOCKED，之后实现、clock repair、独立 ACCEPT | 首个受阻后并未使用 synthetic transcript；后续 source ledger 写入实际 session causal seam。当前 `packages/engine/src/session/causal.rs` 使用真实 causal facts；`observation_clock.rs` 映射采集时间，`decision_chain.rs` 有 lunch boundary 回归。报告应同时保留“不是通用因果估计”的解释、恢复边界与样本局限，不将初始 BLOCKED 当当前未实现。 |
| 1387–1391 | Resolve-blockers Todo 1 evidence qualifications | Tauri helper/actor测试不是 invoke dispatch；Wayland 零字节 PNG 不证明零尺寸协议；Task36 sample unchanged 不证明所有获取时点相同。这些是有效证据限制，未发现本次后续记录推翻它们。 |

### Learnings：1499 行

| 行 | 章节主题 | 后续/当前 caller 追踪 |
|---|---|---|
| 7–47 | Todo4 docs contract、Task39 规模数据、Task35 diagnostics/WASM/Tauri native/Wayland prerequisites | Todo4 checker 有通过记录；Task35 feature gate/STALE guard 与 native prerequisites 有修复段。Task39 29/69/133MB 是样本，旁边“全超 8 MiB”描述陈旧，见摘要。 |
| 49–119 | Task30 WASM Map/null/date、Task32 Tauri events、Task33 shared company cache/RemoteHost 修复 | 当前 WASM serializer、PublicReportSummary、remote metadata cache 对应代码有明确修复记录；date 应由共用 `period_end_date` 供给。 |
| 121–148 | 多 seed baseline、engine_error_events、运行成本、pnpm | baseline 的设备时间只能作原机预算；当前 `scripts/main-release-validation` 有后续 bounded runs。非零 engine_error_events 是报告观察项，不能悄悄改为失败门槛。 |
| 150–244 | 纯移动模块映射、官方会计政策取证、来源 blocked 的纠错方法 | 仍有价值的结构/取证经验；2026-09-10 具体文件行、外网可达性和结论不是现在的 API 或规则权威。乱码行 315 后另列。 |
| 245–336 | Task21 plan 状态机/Expired bug、W1-4 日历材料（315–336 mojibake） | Expired/迟到日终 defect 已以测试和豁免修复；日历乱码不猜。 |
| 337–429 | Accounting、CivilClock 与 SaveSlot 契约 | 任务 27/29 后续修订 save/API；此处字段、初版锚点和 test count 均为历史版本。 |
| 430–549 | Company、analysis profile、industrial accounting | 作为实现背景；简化须使用现行正式 docs/policy-source 裁定。公司与市价隔离仍是领域约束，不把历史固定样本当实时报价规则。 |
| 550–695 | technical/price-memory、bank、experience feedback | 技术历史窗口、RNG 和分层数据面可对照当前模块；`experience.rs` 字段曾因兼容跳过序列化，Task 27 新格式收档后，验证须按现行 schema，不能复用旧字节预期。 |
| 696–843 | real-estate、insurance、consolidation 的模型/简化/金样 | CAS17 保持显式游戏假设；insurance simplifications 曾缺正式登记后补；consolidation 的 duplicate pair 及业务规则有后续修复/登记；总量金样不是外部会计权威。 |
| 844–959 | CompanyOperations、行业事件采样及完整财报/更正语义 | Task14 行业差异必须继续显式：银行存贷不消费需求冲击。Task13 correction 曾有跨期利润漏洞，现由持久 adjustments + lifecycle assertion 修复。 |
| 960–1074 | disclosure/public-library/acquisition、EarlyRead、Appendix/测试经验 | Task29 event 和 Task27 persistence 已收口；APPROVAL_HOUR 单常量。获知幂等、公共曝光不等于个人阅读仍须区分。 |
| 1075–1143 | Attention discovery 权重、Task18 beliefs/valuation | 参数登记的历史 REJECT 在 issues 1055–62 翻转 APPROVE；RNG 独立流规则仍是调用链硬约束。全部使用真实 company/session caller，不从 API 假设取得。 |
| 1144–1212 | Task23 urgency、Task22 allocation、Task24 execution | 修复后的路由语义显式校验数量/股票规则；linked parent / pending events 的外部簿边界在 Task27 有后续持久化清理。 |
| 1214–1274 | Task26 V 删除、decision chain、公司装配、restore 过渡态及锚点 | 任务 27 已取消“经营重放并重置个人状态”的过渡方案；当前 SaveSlot/restore 需看任务 27、28 代码，不可引用此初版行为。 |
| 1275–1309 | Task27–29 save/scenario/types | Task27规模、save-contract 与绑定清理记录；Task28审查 gap 后复验；Task29严格 DTO/public fields。属后续验收摘要。 |
| 1310–1393 | Task30–34 bridge/server/Tauri/Worker UI | Task31 初始 server defects 后由 1364–92 修复记录取代；header-only bearer、timeline/revision、bounded preflight 和 player-only public baseline 是后续现状证据。Task34 的严格错误显示与后续 controlled-date E2E 修复已记。 |
| 1394–1452 | Task34 public-company UI、period-date shared repair、browser evidence/date input | shared PublicReportSummary 期末日期修复已取代 WASM-only idea；日期输入的空 draft 不得回退假装默认。视觉只由新截图/DOM证据证明。 |
| 1453–1485 | Task36 causal diagnostic discovery、source ledger、authoritative clock | 初始字段缺失观察已被 1459 起实现覆盖。仍要限缩结论：真实 session collector，不是合成轨迹；不推断 aggressor；clock 使用 lunch elapsed time；单 seed 未变不代表全场景均无变化。 |
| 1487–1499 | Task36 ignored evidence 的 Git hygiene、Todo1 pinned reconciliation、Todo2 Corepack/Clippy | 是后续记录；产品 tree 的当前完整验证以 10/03 发布验收记录为准。Todo2 自动修复已将旧 clippy 债处理，但不要从本文本身宣称现行全 lint 状态。 |

## 旧阻断复核与新候选

| 旧结论 / 发现 | 后续裁定与当前证据 | 当前状态 |
|---|---|---|
| Task12：`build_worksheet` 重用同一 counterpart line 会重复抵销 | `packages/engine/src/accounting/consolidation/eliminate.rs:66` 找 mirror 时排除已 `used` 项；`packages/engine/tests/consolidation/failures/intercompany.rs:82` 锁重复配对拒绝。 | 已修复；原 REJECT 仍保留为历史。 |
| Task13：更正调整在次期又进入损益；附注溢出吞零 | 持久映射经报告窗口消费；`tests/industry_reports/lifecycle_gold.rs:352–370` 明断后期 P&L 为零且调期初留存；当前 `window.rs:67` 仍接受调整映射。Issues 849–879 为当时复核原文。 | 跨期重复问题已修复。`notes.rs` 原 `unwrap_or(zero)` 是否移除应根据当前实现继续核实；本轮未把它记为新缺陷，因为需确认具体当前分支/调用路径。 |
| Task25：发现权重数值未进正式政策文档，REJECT | issues 1055–1062：事故提交否定、b2d88a9 基底在分支、隔离复验与数值比对通过，APPROVE。 | 已修复并复审。 |
| Task31：吞 civil settlement error、restore timeline/resync、query token、private baseline、unbounded nested allocation | learnings 1364–1392 记录相应修复；`routes.rs` header 鉴权、有限 body、`MAX_NESTED_SAVE_DEPTH=64`；公开 baseline 只投影 player。 | 后续记录证明主要 blockers 已修；旧 finding 仍有回归价值。 |
| Task32：缺 engine typed events/API、desktop native build unavailable | Task29 增加共用 Event/query seam，Task32 Tauri 透传 engine report/events；Task35 learnings 34–47 记系统依赖和 Wry 启动验证。 | 原 shared-contract blocker 已解除；“actor helper ≠ 注册 invoke dispatch”仍是证据边界（issues 1387）。 |
| Task34：WASM Option None/date 失败，浏览器只验证 error | Task30 learnings 61–78、1415–1425 记录显式 null 与共享期间末日修复；issues 1331–1349 和记忆 1427–1451 记录生产成功路线与空日期输入修复。 | 已修；Task34 historical error-only 结果不是最终 UI 验收结论。 |
| Task35：缺服务器 route/Tauri command，GTK/WebKit 使 native 不可验证 | issues 22–25 与 learnings 34–47 分阶段记录 adapter、安装环境、MockRuntime、实际 Wry 启动；task35 debug generation 现用闭包先 guard 后读。 | 功能补齐；以具体证据层区分 Native actor 与真实 invoke dispatch。 |
| Task36：Event 缺订单 ID/取消原因/时序，合成 API 不可信 | issues 1357–1385 与 learnings 1459–1485 记录从真实 matcher/auction/session source 收集、修 clock、两旧 save hash 重建及 ACCEPT；当前 `session/causal.rs` 暴露由真实 facts 派生报告。 | 原 blocker 已过；仍是观测性诊断而非已识别因果估计，事件含义/跨恢复边界应按报告明示。 |
| Task39：20k/50k/100k 都高于“不变的 8 MiB”，不可远程 raw JSON load | 现行 route body limit=`MAX_SAVE_DECODE_BYTES + 1 MiB`，Engine 解码上限 512 MiB；另 10 万完整日 591,344,527 bytes 超 536,870,912 bytes 的 release 记录。 | **原数字/“当前 8 MiB”结论过时；新容量上限是真实且有限。** 旧的 29/69/133 MB 都在新 bytes 门内，但本轮未重跑 HTTP 大档端到端，不据此声称部署通过。 |

## 证据债与复核边界

- `issues.md:243–267` 和 `learnings.md:315–336` 有明显 mojibake。原始内容可作为历史字节保留，但无法在本报告中准确引用其含义；禁止用邻接章节猜出复原文本。
- 历史“某建议缺少测试”条目（Task8/9 review 次要观察、Task16 O-1/O-2 等）并非都能由当前全量通过推导已补专门测试。本轮把它们保留为覆盖候选，没有声称已修/未修；下一次针对性测试审查应逐 case 搜索。
- 多处记录曾声明游戏简化/版本化参数仅登记在代码头注/notepad，后有 docs+fixture-only review 修复模式。正式是否闭合需核对当前 `docs/company-accounting.md` 与 policy source fixture；历史工作文档不能替代正式领域规范。
- 10/03 release summary 是发布验证记录而非本次实际运行测试；本任务没有运行测试。引用它只用来核对已有后续事实。

## 评审回答

1. **领域语义：** 本次审查不改产品语义。收录的 A 股语义边界（100 股整手、买卖约束、价格笼子、市场分类）只按已有独立复核/当前调用链表述。行业会计条目是游戏简化说明，不作新的法规合规背书。
2. **必要性/范围：** 唯一新增内容是应复核任务要求形成的完整全文审计记录。唯一可行动的文档总账候选为 Task39 8 MiB stale claim 与更新后 512 MiB capacity evidence 的同步关系；其余均作为已完成、历史记录或证据限定。
3. **遗漏/跨层/复杂度：** 未发现仍未解决的历史 REJECT 可直接定为当前产品 bug。保留需关注的跨层边界为 bounded Server restore vs 大 Engine JSON 存档、Tauri actor 测试 vs 真 invoke dispatch、诊断 observation vs causal estimate，以及政策 fixture vs 正式 docs 的登记闭环。
