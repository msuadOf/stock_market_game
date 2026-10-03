# sweep35：Task 8 存档验收与 Task 9 历史语料证据复核

基线为 `b76ece3` 产品；当前 worktree merge HEAD 的 `packages/`、`apps/`、`scripts/`、ADR-0025 与测试清理决定相对该基线无差异。已按本轮此前读取的 `AGENTS.md`/`docs/principles.md` 执行。只新增本记录，未改产品/Git，未跑测试或长验收。

## 全文读取

以下三份文档从首行连续读至末行，共 **92 行**，没有只读标题/摘要：

| 原文 | 行数 | 范围 |
|---|---:|---|
| `.omo/evidence/escrow-parallel-engine/task-8/acceptance-map.md` | 33 | 1–33 |
| `.omo/evidence/escrow-parallel-engine/task-9/corpus-diff.md` | 9 | 1–9 |
| `.omo/evidence/escrow-parallel-engine/task-9/historical-witness-audit.md` | 50 | 1–50 |

## Task 8 逐条验收映射

| 原文行号/条款 | 当前代码与测试证据 | 状态/限制 |
|---|---|---|
| acceptance-map:3–8，基线PASS与29/29、14/14、33/33 | 原报告明确基线 `94f337e`，且 save_contract/verification_evidence 复用历史结果 | 历史运行证据；本轮不重新认可为现行全部PASS，不重复运行 |
| :12，schema v2/旧档/缺失/未来拒绝 | `session/persistence/v2.rs:97` 先解头、:109分类拒绝；`v2_tests.rs:357`、:370；`tests/save_contract/failures.rs:108`、:120 | 生产实现仍在；旧 `save_restore_surface_rejects_legacy_schema_before_restore` 已随语料投影族退役，不是少了版本拒绝 |
| :13，初始/market tick/CivilUpdate/恢复后静默点 | `tests/save_contract/main.rs:188` 仍调用内部GameSession::save并比对立即重存；`session/failure.rs:85`内部保存 | 内部恢复测试仍有；**公共日内保存契约已由ADR-0025取代**，不能把这些内部静默点当用户可保存入口 |
| :14，全量权威状态往返 | `tests/save_contract/main.rs:212` 非空belief/information/price-memory再字节比较；`v2_tests.rs:404` 完整StrategyState；`v2.rs:372`恢复 | 已实现；新存档精简字段不要求恢复旧展示镜像 |
| :15，同seed续跑三天逐tick字节等价 | `tests/save_contract/main.rs:264` 当前短fixture固定每交易日1 tick，:279事件比较、:287日终save比较、:293次日首tick比较 | 固定受理事实/受控fixture覆盖保留；原“三天逐tick”是历史测试叙述，不外推自由调度整局确定性。当前工具的自由调度错误门禁另归已有G39 |
| :16，真实非空Sell与envelope恢复 | `v2_tests.rs:728` 公共enqueue/step生成成交与保存；:680零现金卖单恢复；内部订单恢复经 `session.rs:2716`、:2720 校验再构造候选 | 业务覆盖仍在；`real_save_v2_live_sell_projects_restore_and_continuation_evidence`专用projector已获批删除。公共加载日内live order现应拒绝，见ADR-0025:30 |
| :17，receipt cursor/seen prefix | `v2_tests.rs:420` 非空envelope/prefix往返；`pipeline/retail_projection_persistence_tests.rs:145` 旧收据不重消费/新收据继续；`v2.rs:136`游标一致、:247–275身份校验、:438–441原子安装 | 实现与测试定义仍在 |
| :18，跨tick累计费用与顺序 | `v2_tests.rs:560`、:586、:829分别核累计audit/逐腿priority/同tick债务；`v2.rs:664`费用恢复计算，生产 `session/pipeline/transition.rs:156`–:183累计增量 | 已实现；名义/实收费历史保留，最小存档重建可派生字段 |
| :19，零现金Sell与#9封顶 | `v2_tests.rs:680`、:728；`account.rs:420`–:422及pipeline费用增量路径 | 已实现；仍是获批游戏简化，不宣称交易所真实清算 |
| :20，跨层损坏拒绝/不部分恢复 | `v2_tests.rs:1043` receipt gap、:1120 envelope tamper、:1137 poisoned/unknown fields、:1258 attention drift；`v2.rs:228`验证、:372先验证后准备、:411候选账户、:438一次安装 | 核心实现存在。旧 `complete_restore_rejects_tampered_snapshot_reservations`符号现不存在；snapshot预留镜像精简后不应恢复旧字段测试，现 :440 明确save不写镜像而保留实收费 |
| :21，竞价余单/arrival序恢复 | `tests/auction.rs:759` 中途恢复、:883卖出股份跨订单与恢复；内部恢复入口同上 | 内部能力仍在；公共日内竞价存档由最新ADR排除 |
| :22，空档/重存字节/hash/续跑伪造负控 | 搜索 `save_restore_surface_*` 专用证据测试已不存在；清理决定 `docs/test-cleanup-checklist.md:85` 删除SaveRestoreLiveOrderInput及语料投影测试 | 用户批准退役，不能把旧负控工具消失列成现行产品未实现 |
| :24–28，大A语义和仅证据范围 | 保存恢复保留T+1、费用、个人状态；现行ADR-0025:25要求继续保留；交易规则#9简化仍登记 | 未发现该三文档范围引入新语义遗漏；本轮只复核，不重新查官方网页 |
| :30–33，未跑全回归/release/K7/perf | 原文明确不推导完整验收；本轮同样未运行 | 历史定向PASS不能核销长期/三宿主/性能证据债 |

## Task 9 两文档逐章映射

| 原文条款 | 现行依据/代码 | 判定 |
|---|---|---|
| corpus-diff:1–5，9/10 INCOMPLETE、九面精确比较、三种mutation拒绝 | 原始 `corpus-diff.incomplete.json`仍在；旧compareCorpusCase/normalizeUpdates等已按清理§12移除 | 保留历史未完成结论；不把旧9/10改写为现行PASS，也不要求重建被批准退役栈 |
| corpus-diff:7–9，zero-cash旧witness缺失与禁止制造证据 | `docs/test-cleanup-checklist.md:88`明确历史密封数据保留、不再可执行复验且用户接受；总账 `coverage/h01.md:16`已核销为旧证据限制 | **不是新增代码待办**；当前零现金Sell业务通过production/v2测试核查，和旧引擎接受见证是不同主张 |
| historical-witness-audit:3–11，2026-09-23审计基线/搜索政策 | 原报告只允许旧端既有零现金已受理产物，不制造fixture | 合法历史审计政策；本轮未重新枚举所有原始JSONL或Git对象，不声称核验257条原始产物 |
| :13–34，attempts01–12、4个sealed、257/224/33/70统计和TSV hash | 三文档所列统计为当时历史结果；本轮读取关联report和现有incomplete路径 | 不把临时TSV缺席当代码遗漏，不把new-engine-only stress冒充old-side evidence |
| :36–43，Git可达历史无捕获 | 历史报告明确当时git log/rev-list结果 | 不能拿2026-09-23“all”断言当前全部refs；本轮不据此宣称新Git搜索仍无witness |
| :45–50，missing surface保持open、不弱化 | 较晚2026-09-28清理决定取消可执行旧验证栈，`coverage/h01.md:15`–:17已解释适用关系 | 历史阶段仍INCOMPLETE；当前工具范围已改，不重新开放旧补见证任务 |

## 现行 step / 证据 / 负控 caller

- `session/failure.rs:29` 的普通step走 `step_inner(false)`；:38的显式step_with_commit_evidence走同一 `step_inner(true)`；:73进入 `pipeline::execute_authoritative_tick`，错误:76调用poison_failed_step；证据是观察接缝，不是另一旧引擎执行器。不能因为旧corpus replay被删除就报告当前step缺实现。
- `session/persistence/v2.rs:122` capture先require_healthy，:130核envelope完整证据，:136核游标；:228深度校验、:372原子恢复，存在实际存档调用而非纯占位。
- `verification_evidence.rs:197`保留守恒投影，:634 UpdateStreamProjector、:681 project_update和validate_update_transition保留；`verification_evidence/tests.rs:237`独立来源顺序合法且损坏local chains拒绝。删掉的是历史语料投影族。
- `scripts/simulation/run-escrow-verification-matrix.mjs:15`实际import verifyConservationSnapshot，:479要求负控有绑定rejection/rollback/control witness，:486核守恒snapshot；当前负控并未被全部删除。`escrow-verification-contracts.mjs:178`保留perturbation helper，但多数helper仅测试消费属明确KEEP决定（测试清理:87），不报生产接线遗漏。
- `packages/engine/examples/escrow_verification_harness/runtime.rs:107`仍保留corpus_projection键，:331固定None；:1026测试核None；`committed.rs:176`同样固定None。这个null按清理:85是JSON键集契约，不是待填功能stub。
- 公共日终存档以 `docs/decisions/0025-day-end-only-persistence.md:12`–:33为最新边界：完整CivilUpdate后候选、不允许日内live订单/escrow档。Task8内部静默点测试不抵消公共边界，也不说明公共入口漏接。

## 候选与反证结论

本范围 **没有新增确定代码遗漏**。看似遗漏的旧zero-cash witness、空档/authority/continuation负控projector、reserved镜像测试和公共mid-auction持久化，分别由测试清理§12、ADR-0019精简及ADR-0025取代；均已由现总账H01/R10/S06覆盖。

`docs/open-questions.md:10`仍写“须补齐密封旧语料与新路径比较”，与后来的测试清理:88适用范围存在旧文本差异；总账H01已明确核销，故这是可以后续修订的历史入口措辞，**不能升级为新产品代码缺口或擅自恢复工具**。现行G39自由调度确定性门禁与Q05持续测试发现入口仍独立有效，本组三文档没有新证据核销它们。
