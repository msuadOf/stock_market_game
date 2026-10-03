# Sweep 33：Escrow Tasks 10–12 历史证据全文追查

## 阅读边界与口径

- 连续全文读至 EOF：`.omo/evidence/escrow-parallel-engine/task-10/divergence-audit.md` **28 行**；`task-11/execution-log.md` **299 行**；`task-12/validation.md` **23 行**，共 **350 行**。Task 11 包含全部历次失败、作废与最后 closeout，未将中途失败当作现行实现状态。
- 根 AGENTS/principles/architecture 已在 sweep15 读过，本批沿用同一守则。目标 `b76ece3`，此前已确认合并 HEAD 所查产品源码相同。本批只静态读源码和文档，未跑测试、构建、矩阵、Git 写操作或产品；本文件为唯一新增工作文件。
- 对照总账 G38/G39、R14/R20、旧 `reaudit-tools.md`，并读 `.omo/plans/escrow-parallel-engine.md:288` 起状态/用户收尾决定及现行 `docs/testing.md:193` 实际受理语义。历史 PASS 绑定当时提交与指纹，不能作为当前基线测试通过声明。

## Task 10 divergence-audit 全章节

| 原文 | 当前证据与状态 |
|---|---|
| L1–8 日期及 numeric expectation 未改、delta 空表 | 是该任务固定历史声明，不通过当前空 working-tree diff 伪证。当前 `scripts/simulation/audit-diagnostic-divergence.mjs` 仍提供有范围诊断审计能力；本文未替历史执行该工具，也未补造 sha9/sha10。 |
| L10–13 causal example 从 step() 改 step()? | `packages/engine/examples/causal_diagnostics.rs:14` 当前仍 `session.step()?`，错误继续传播，不吞 StepFatal；已实现兼容修正。 |
| Boundary limitation L15–22 无 Task9/Task10 完成边界 SHA | `.omo/plans/escrow-parallel-engine.md:294` / `:295` 保持 BLOCKED；`:305` 用户明确终止迟补历史见证，未把缺证改 PASS。属于被用户停止的历史验证范围，不是应新增生产代码，也不能要求制造完成提交。 |
| L24–28 初始持仓没有 game buy identity，卖出不误写失败经历 | `packages/engine/src/experience/position_transition.rs:90` 以 has_buy_experience 限制卖出失败/获利调整；`:105` / `:109` 保存/清空卖买身份；`packages/engine/tests/experience_feedback/main.rs:250` 有 seeded holding无买单失败事件回归源码。真实成交仍走 settlement projection，不因旧例外说明认定当前发生身份缺失。已有G08涉及dated衰减链，不在这里重编号。 |

## Task 11 execution-log 全章节

| 原文区段 | 当前实现、后续反证与状态 |
|---|---|
| L5–49 v4 qualification、74 tests、fingerprint、正式首波CPU | 历史资格验证，不是当前通过。`scripts/simulation/baseline-run.mjs:129` 按总预算算rayon/child，`:1064`共享executionPool启动子进程，`:1074`传RAYON_NUM_THREADS；有实际并行入口，不把线程配置数当实测active。该v4正式根后续作废。 |
| L51–58 v4 formal roots作废 | 后续v5/v6/v7替代，旧800% CPU不足128核已被记录且中断；不据旧资源不足认定当前仍固定8 seed或单核。 |
| L60–93 v5 30-child bounded probe且无manifest | 明确qualification-only，不能计入94。当前 `baseline-run.mjs:1380` 强制完整seed列表，`:1415` finalized dimensions才发布sensitivity；root verifier `verify-simulation-artifacts.mjs:496`起累计canonical/rerun。未读取失效scratch当正式证据。 |
| L95–144 v5 freeze/formal launch、重复启动作废 | 当前 `baseline-run.mjs:683`源码清单，`:1255`拒不存在的resume根，`:1261`取Git/指纹，`:1267`构建后再核对；身份绑定保留。不以旧双启动推导当前有多个完整验收批次运行。 |
| L146–180 两小时失败、六小时resume | L182–189后续明确永久invalid；当前 `baseline-run.mjs:24` / `:25` 硬300000，`:144` / `:147`拒超上限，旧ENV不再参与超时设置；不要求重现旧6h命令。 |
| L182–189 用户时间限制裁决 | 当前源码与 `docs/testing.md:99` 为10s普通/300s长验收；K7留1000ms清理。正式命令需进程外supervisor是调用职责，不能把runner仅有内部timer自动判为业务漏实现；本轮不启动命令。 |
| L191–213 v6代表性资格、94矩阵、短长分类、共享deadline | `packages/engine/examples/simulation_baseline_fixture.rs:86` 输出bounded profile，不伪装全规模；`baseline-run.mjs:1384`每套共享deadline、`:1392` / `:1417`发布前受deadline约束；`:756`独立多核构建。`docs/testing.md:119`明确允许独立构建阶段和执行阶段各自5min，因此prepare/execution两个deadline不是擅自把同一执行批次放宽到10min。 |
| L215–256 v6 formal失败：fee reserve超过现金、12份partial不计、seed7独立panic | 当前 `plans/allocation.rs:44`校验funds，`:53`validate_request仅非负/shape，`:76`买入请求额+费额然后min remaining，不再把fee reserve必须小于总现金作为合法性条件；`session/decision_chain.rs:988`真实调用allocate_child_quote_budgets，`:975`卖出fee_reserve ZERO。历史特定拒绝条件已被移除，不复报seed7旧panic。合法请求仍可受余额约束，G38新旧计划分配同批问题是另一个既有缺口。当前是否完整复跑seed7未验证。 |
| L258–299 v7 postcommit closeout、94/94、二进制hash、构建单列 | 历史绑定f222417与5d3c…，当前fixture名字已为simulation_baseline_fixture（Cargo.toml:47），`baseline-run.mjs:742`解析该Cargo产物，`:788`源指纹嵌入、`:1082`校验输出指纹、`:1084`raw字节hash。正式独立verifier已演进为`verify-simulation-artifacts.mjs`，不要求恢复verify-k7-root旧名。历史94/94不代表当前并发受理规则下新的工具契约正确；新增C33-01见下。 |

## Task 12 validation 全条目

| 原文 | 当前证据与状态 |
|---|---|
| L5–9 symbol tests/PASS14/两处seal_allocation_snapshot | 当前 `scripts/check-doc-symbols.mjs:6`收集explicit engine::符号，`:25`明确lexical guard不冒充Rust公共路径检查。旧seal_allocation_snapshot已重命名：`docs/architecture.md:111` / `docs/trading-rules.md:7`写DecisionResourceSnapshot::seal，当前 `session/pipeline/decision_resources.rs:93`该类型seal承担P1，不按旧名字消失判生产漏接。历史14条计数不重写成当前计数。 |
| L10–11 panic进程级，不可恢复tick | `README.md:12`及`docs/architecture.md:123`仍明确，生产错误类型与panic语义未被文档掩盖；已同步。 |
| L12–14 wiring review与官方content review分开、escrow/#9费用收取为简化 | 当前`docs/trading-rules.md`保留交易简化；收据应计/实收由`session/pipeline/transition.rs:150` / `:171`计算，卖方`:219`起按本次gross封顶并追收，不把工程接线证据当官方清算规则。 |
| L15–17 普通10秒、长批300秒、多核、build单列 | 当前AGENTS/testing与`baseline-run.mjs:24` / `:129`资源策略一致；相关执行记录不能证明当前实測多核，无本轮验证。 |
| L19–23 Task11历史94/94，Task9 witness/Task10 commit boundary诚实未完成 | `.omo/plans/escrow-parallel-engine.md:294` / `:305`最后用户收尾决定仍在，已明确不迟补，不当作漏代码、不冒充PASS。 |

## C33-01：G39还遗漏第三个K7统计矩阵自由调度比较入口

- 原文Task11 L193–195/L284–286把9次deterministic rerun计入94；这是历史当时语义。现行`docs/testing.md:193`–194说明seed不含实际到达轨迹，两次自由调度可产生不同同价排队、成交账户与后续计划。应按最新规则判断工具，不要求生产恢复跨运行全局排序。
- 当前总账G39与`reaudit-tools.md`只列 `run-escrow-verification-matrix.mjs` 和 `escrow-verification-contracts.mjs` 两个严格完整artifact比较入口。这里发现统计after/sensitivity另一个调用链仍用同样过严前提。
- `scripts/simulation/baseline-run.mjs:1275` finalizeSimulationMatrix对最后seed重新capture；`:1284`从头执行相同seed/参数、无受理事实重放输入；`:1286`要求整个raw stdout SHA-256与canonical相等。`:999`及独立verifier `verify-simulation-artifacts.mjs:427`再次强制canonical/rerun两个hash一致，否则拒绝正式manifest。
- 真实fixture `simulation_baseline_fixture.rs:49`建立新session并`:187`自由step，另`:52`重新run_price_volume_baseline；`:76` / `:77`把价量与causal report写入完整stdout。`diagnostics.rs:798` / `:804`同样new+step；报告统计成交结果，受合法调度差异影响，不仅纯输入计算。生产`local_admission.rs:264`将真实gate arrival边纳入局部DAG，独立股票输出布局不定义交易优先级；该脚本未固定这份真实受理事实。
- 反证检查：相同budget不意味着相同实际arrival；仅比较统计而非完整SaveSlot也不保证自由调度输出一致。历史f222417那一次通过只能证实当时特定输出相同，不是当前正确契约证明。没有据此声称本轮实际运行必失败。
- 分类建议：**扩展既有G39覆盖范围**，加入baseline-run finalizer与独立root verifier第三条生产验收链；不新增重复编号，不删守恒/价时/依赖/失败负控。9次rerun数量要求与最新重放输入的关系需收口，不能直接删除9次检查来规避失败。
- 本轮未新增测试；代表性短验证应区分自由调度有效结果与固定受理事实重放，并验证manifest/verifier都接受新的正确契约而仍拒绝损坏hash/身份。

结论：旧资源/超时/seed7失败已有后续解释或源码反证；Task9/10缺证已由用户明确停止迟补。新发现是G39少记统计K7 finalize与root verifier消费链，其他本批条款未确认新增产品断链。本记录不重新授予历史PASS或宣称无遗漏。
