# luna35：Escrow Task 8/9 全文复核

日期：2026-10-03。复核工作树 `/data1/baiyifan/workplace/stock_market_game/.worktree/implementation-reaudit`，当前 merge HEAD `a7c7ce357bdc9f88c03633744b2d5815db49e9b2`；父任务指定产品 `08e4fc7`（merge 同）。已读根 `AGENTS.md` 与 `docs/principles.md`。只新增本审计记录；未改产品或 Git，未运行测试、编译、长验收。

## 全文读取及章节矩阵

三个指定原件均从首行连续读至 EOF：`acceptance-map.md` 33 行、`corpus-diff.md` 9 行、`historical-witness-audit.md` 50 行，共 92 行。下表按其全文覆盖，不把旧执行结果说成当前运行结果。

| 原文行/章节 | 原文主张与旧结论复核 | 当前代码、caller 与新决定 | 判定/反证 |
|---|---|---|---|
| acceptance-map:1–8 | 基线 `94f337e` 的历史 PASS；29/29 是核心 v2 smoke，14/14、33/33 明确复用先前结果 | 不重跑且不能推导当前整包通过；`test-cleanup-checklist.md:81–91` 记载后续旧栈清理验收，也不把历史数字延伸到当前 HEAD | 旧 PASS 仅按报告范围陈述，没找到当前验收遗漏 |
| :10–12 | v2 schema、legacy/missing/future 拒绝 | `persistence/v2.rs` schema header 分类仍在，`v2_tests.rs::schema_header_accepts_only_explicit_v2` 与 save contract 错误用例仍存在；旧 `save_restore_surface_*` 证据符号已删 | 版本拒绝仍由现行 typed save 校验覆盖；不要求复活 corpus 专用 verifier |
| :13 | 初始、market tick、CivilUpdate、恢复后 quiet points 均可恢复并逐字节重存 | `save_contract/main.rs` 仍有内部静默点测试；公共存档时点受 ADR-0025:12–18 限制为完整成功日结，日内检查点仅作内存用途 | 旧“market tick 是公共持久化点”不能照搬；内部 checkpoint 与持久存档没有语义混同 |
| :14 | 完整权威状态往返 | v2 `SaveRuntimeV2` 仍校验和原子恢复策略态、ledger、receipt seen-prefix；保存派生字段精简按 ADR-0019/0025 仅需保留继续运行的权威事实 | 业务恢复链仍在；不要求回填退役镜像/旧格式兼容 |
| :15 | 同 seed 恢复后逐 tick 事件及日终存档一致 | `restore_is_byte_continuous_with_uninterrupted_run` 测试源码存在；它不能证明任意自由调度下完整历史确定性。独立自由调度问题仍登记 G39 | 原有历史结论未被扩大；不是 Task 8 新漏项 |
| :16–17 | 非空 live Sell/envelope 恢复及 receipt prefix 连续 | 内部 `GameSession` v2 恢复和真实 step 测试源码仍有；公共 `ProtocolSession::restore` 按 ADR-0025:30–33 拒绝日内订单、母单、冻结 envelope；receipt cursor/seen-prefix 的恢复校验仍在 v2 与 projection 测试中 | 清楚区分内部验证/回滚能力与公共日终档；旧 public live-order 主张不再适用 |
| :18–19 | 跨 tick 费用审计、零现金卖单及 #9 封顶简化 | `v2_tests.rs` 的 cumulative audit、per-fill priority、zero-cash seller、real-step 测试保留；实现沿 session transition、account/settlement。#9 是已登记游戏简化，不声称真实清算规则 | 当前 A 股术语/资产单位未见漂移；证据仅是源码存在，本轮未运行 |
| :20 | tamper、receipt gap、poison、策略 drift 显式拒绝且恢复原子 | `v2.rs::validate_runtime_v2` 先交叉校验；`restore_runtime_v2` 先构造 ledger/seen/strategies/accounts，最后统一安装。对应 tamper/gap/poison 测试符号可检索到 | typed 错误与原子安装仍是现行行为；已删除的 snapshot-reservation 镜像用例不能据此称为漏测 |
| :21 | 中途竞价余单/arrival order 恢复 | `tests/auction.rs` 仍测内部恢复；公共日终档不承诺日内竞价状态 | 旧公共中途存档范围被 ADR-0025 取代，内部模拟状态恢复没有因此退役 |
| :22–23 | 空档/authority/续跑伪造负控及证据范围 | 专用 `save_restore_surface_*` 测试属于清理 §12 删除的 escrow projection/evidence 栈；现行 v2 tamper/gap 测试仍负责存档不变量 | 旧负控消失是批准退役，不等于所有恢复负控消失 |
| :24–33 | 大 A 语义、改动范围、未跑全回归/release/K7/perf | 三份是历史证据说明；ADR-0025 不改交易规则但限定存档面；本轮不运行测试 | 接受旧报告的验证边界，不把定向 PASS 冒充本轮 PASS |
| corpus-diff:1–5 | Task 9 为 9/10 INCOMPLETE；九构造面及三 mutation 拒绝当时通过 | `corpus-diff.incomplete.json` 留存历史 receipts；§12 明确旧 compare/replay/projection 代码退役，密封原件仅留档 | 历史“不完整”仍真；工具退役后不要求重造 9/10 PASS |
| corpus-diff:6–9 | `equivalence:normal-multi-leg-terminal` 缺旧侧零现金接受 witness，禁止造证据 | `historical-witness-audit.md:13–50` 是 2026-09-23 当时的枚举与结论；清理 §12:88、H01:15–16 明确不再可执行复验并接受历史缺口 | 不是现行产品代码任务，不能把旧 witness 缺失改称已补齐 |
| historical-witness-audit:1–11 | 审计日期、sealed artifact 证据规则及不造 witness 政策 | 当时审计政策与旧 corpus gate 匹配；后来用户确认退役验证栈，证据保留但复验要求取消 | 原审计仍作为历史材料有效，规范适用已被新决定收窄 |
| :13–34 | attempt 01–12、sealed 数、257/224/33/70 配置与本地 TSV hash | 本轮没有重新枚举 257 个 JSONL，也没有读取/校验那个临时 TSV；不可声称这些统计是本次验证 | 统计仅为原审计时点事实；其临时表缺失不构成生产实现遗漏 |
| :36–43 | Git 可达对象中没有 witness/语料 | 原文明确是 2026-09-23 的 `git log --all` / `rev-list --all` 结果；本轮没有重做搜索或写 Git | 不把历史搜索结论外推为当前所有 refs 的结论 |
| :45–50 | 零现金旧 witness 缺失仍 open，不弱化条件 | H01:15–19 与 cleanup §12:81–88 是较新的处置；保留 A 股业务资金/股份语义与对应生产测试，不再要求旧引擎等价比较 | 旧任务阶段 incomplete 与当前不需重建旧工具可以同时成立 |

## 现行接缝追踪

- **step caller**：`session/failure.rs::step` 与 `step_with_commit_evidence` 共用 `step_inner`，后者进入 `pipeline::execute_authoritative_tick`。提交证据是观察/验证接缝；不构成第二个历史 engine，也不依赖旧 corpus replay。错误路径进入 typed fatal/poison 状态。当前真实 caller 在协议层 `ProtocolSession::step_frame[_with_commit_evidence]`，harness 的 `runtime.rs` 也报告相同 authority path。
- **typed restore**：外层 restore 先构造并校验基础 `SaveSlot`/订单，再调用 v2 验证和恢复；v2 校验不清空损坏状态，完成所有可失败转换后一次性安装 accounts、ledger、seen-prefix、cursor。源码锚点为 `persistence/v2.rs::validate_runtime_v2/restore_runtime_v2` 与 `session.rs::restore`。公共 Civil/ProtocolSession restore 再落实日终契约；内部 GameSession 快照能力不能误读成用户可持久化中途档。
- **证据投影**：`verification_evidence.rs` 仍保留 conservation snapshot、`UpdateStreamProjector`、`project_update` 和跨 update 连续性校验。退役的是 corpus surface/witness 抽取、旧语料适配器及 bundle 装配。现行 harness 报告 `corpus_projection: null` 是 cleanup §12 明示保留的 JSON 键集占位，不是遗漏实现。
- **负控 caller**：当前 Node matrix 直接调用 `verifyConservationSnapshot`；`validateExecutorEvidence` 对 negative-control 先要求 `negativeControlDetected` 和对应 `disabled_merge`，验证 typed rejection、rollback 字节一致、零发布事件；随后入口再次核验负控结果。不能把此并发/执行器负控说成旧 Task 9 对照器仍在，也不能把旧 save_restore 专用假证据负控说成仍有生产 caller。存档跨层拒绝由现行 v2 typed 测试负责。
- **最新退役决议**：`docs/test-cleanup-checklist.md:81–91` 是 2026-09-28 用户确认全清的 §12：删除 sealed corpus adapter、replay、projection、历史 bundle assembler；保留密封文件不可执行复验的边界；明确多数格式 helper 仅测试调用，只有 conservation snapshot 在 matrix 生产路径。ADR-0025:12–33 则是独立、较新的公共持久化范围，拒绝公共日内订单态存档。二者分别处理测试工具与产品持久化，不能相互扩大含义。

## 旧结论与遗漏候选复核

旧审计 sweep35 的主结论“没有新增确定代码遗漏”经本轮复核仍成立。证据完整性上需保留其谨慎措辞：它说 historical-witness-audit 的 257 条统计和 Git 搜索未在那轮重跑；旧 PASS 不能证明当前 HEAD 全量通过。现行生产层未发现由三份历史材料揭示、且未被新政策或当前测试替代的产品遗漏。

曾可疑的遗漏候选逐项反证：旧 9/10 证据面不完整，但 corpus 工具整体已按 cleanup §12 退役；日内 quiet point/live order/竞价恢复只可用于内部状态，不是公共保存承诺；旧 save/restore 伪造证据负控已由有效 v2 typed 拒绝覆盖存档不变量，matrix 自身负控 caller 仍在；被删的 `corpus_projection` 能力不要求保留，capture 中 null 键是兼容格式决定；金额/股份、T+1、费用优先级及零现金卖单语义仍属生产行为，未因删比较工具而取消。

唯一相关文档漂移候选是 `docs/open-questions.md` 的历史语料比较表述（已由 H01 标记相对 §12 过时）。它不构成代码遗漏，本轮不擅改正式文档。G39 的自由调度 determinism 门禁及 Q05 测试发现策略仍由现行审计总账单独跟踪，不能让 Task 9 退役结论顺带核销。

结论：本范围没有新增有效实现遗漏；A 股语义无新变化。历史 Task 9 仍应称旧阶段 INCOMPLETE，而不是补成 PASS；当前不要求复活已退役验证栈。未运行测试/编译，以上仅为静态源码与文档审查结论。
