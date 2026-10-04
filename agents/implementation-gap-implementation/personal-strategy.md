# 个人策略、经历与 DEV 因果接线

## 范围与依据

- 本轮处理 G08/G09/G37/G38/G42/G43；G07 由 `implement_retail_beliefs` 独立实施并接入同一 capture。未改总账、README、Git index，未提交。
- 阅读根 `AGENTS.md`、`docs/principles.md`、ADR-0013/0016/0026、实施总账、`reaudit-engine.md`、原证据 `exhaustive-review/luna03.md`、`exhaustive-review/luna21.md` 与正式公司计划 K5/K5a/K6/K7。较新决定优先。
- 这是游戏个人认知/策略接线，不修改 A 股撮合优先级、T+1、申报单位、费用、合法价格带或交易时段。未新增交易制度，不以行为参数冒充现实经验校准。
- Q02 主动读取历史的获知边界、Q11 更正/真正违约的生产 cause 选择和 Q25 两个 profile 的身份契约仍未推断实施。scope 选择已按用户确认补充 ADR-0016：仅从本人已知集合选最新期间，同期间 Consolidated 优先，否则 Standalone，同 scope 再比较版本；中期与年度基准必须同 scope，不能混口径。这是游戏选择 policy，不是 CAS 33 强制投资估值规则。官方核验日期为 2026-10-04，依据及适用日期见 [report-scope-policy.md](report-scope-policy.md)。

## 生产 caller 与状态 owner

| 条目 | caller → owner → consumer | 实施边界 |
|---|---|---|
| G08 | `capture_decision_snapshot` → dated 持仓观察/`failure_influence` → sealed `DecisionAccountInput`/Retail 策略；`apply_session_settlement_transaction` → `project_retail_receipts_dated` → dated 成交事实 | 新局持仓 dated 初始化由共享 session owner 接线；初始分配不是真实买入。衰减仅作用于派生输入，历史失败计数与事件不删。机构既有衰减不重复施加。 |
| G09 | 本人 root/散户分析 → 统一 `preferred_own_report` → `BeliefBook::apply_material` → 中期同比增长修订及个人估值 | 中期补充同 scope 的本人最新完整年报；不对中期利润/现金流年化。缺同 scope 年报显式 `NoOwnAnnualMaterial` 且 Book 不变；期限重估保留中期来源 ID。Consolidated 缺归母净利返回 `ConsolidatedNetIncomeAttributionUnavailable`，缺可靠归母现金流返回 `ConsolidatedCashFlowAttributionUnavailable`，不 fallback 或猜分摊；Web strict reason parser 同步。已使用的 NewMaterial ID 不重复修订，direct cause 保留原分支。 |
| G37 | root 初始化 DEV trace → 真实生命周期、预算 grant、`PlanRouteOutcome` → trace enrich | 订单 ID 只来自真实 Accepted/Canceled；拒绝不伪造成交。记录状态/版本/目标/成交进度等实际变化，预算字段来自实际 `AllocationConstraint`，不借终止状态伪装预算。 |
| G38 | root 封存原有 active PlanId → 生命周期 → 同账户 classified 预算请求 | 减仓优先；原有计划续行与本轮新建机会区分，下一次本人观察变成续行。在途冻结/费用、资金来源不变，不设置跨账户撮合优先。 |
| G42 | root 发现前清理 → 真实生命周期后的 AccountExecution 修剪 → P9 实际 receipt dirty owner 修剪 → 成功日结最后维护；`validate_save_slot` → 保护集检查 | 保护集为真实持仓 ∪ 实际 active 计划，其外最多 8 条；不使用旧 frozen 计划做最终 prune，不在 route resume 过早修剪。P9 在 hash/next-observation 前只处理实际成交影响的个人 owner，无成交不全户扫描；合法数量只读检查、不 detach 历史。日结维护位于 rollback 边界内。修剪不删除本人历史公开材料。 |
| G43 | 持仓/关注/active 计划/本轮发现 → root candidate → 获知/分析/生命周期 | 不把所有历史 belief key 自动作为候选；淡出后旧 entry/信息保留，新消息须重新发现才能获知。 |

`RootReadContext.library` 与 G16 owner 协调改为 `Arc<PublicLibrary>`；PlanBook 内部共享历史优化由 G16 owner 实施，本记录不独立声称性能测量通过。

## DEV wire

- Rust trace `tick` 输出无损 u64 十进制字符串；新增 `plan_changes: Vec<String>`。
- Web `npc-decision-trace.ts` 严格同步字段和 canonical u64 边界，拒绝数字、前导零、负数、科学计数和越界，不转 `Number`。
- `NpcDecisionInspector` 直接显示字符串 tick、真实计划变化和真实预算约束；普通产品快照、存档和默认 release 不广播私有 trace。

## TDD 与短测记录

- G43 首个有效 red：先让本人 root 形成历史 belief，清空资格集合后旧 `root_candidate_codes` 仍返回旧股票；断言 `candidates.is_empty()` 失败。修正候选资格后 green。
- G38 red：所有请求按 ExistingPlan 处理时，较早新机会 PlanId 排在旧计划之前；真实 red 显示 `PlanId(0) != PlanId(1)`。恢复分类后 green；补有限现金只够旧计划一份的 grant/constraint 断言。
- G42 red：生产 root 未调用价格记忆 prune 时 9 条未保护记录保留，`count <= 8` 失败。复核补充真实 root → 生命周期 → 执行同 minute tie 用例，旧 frozen prune 错删新 active plan 记忆，真实 red 5.31 秒（`personal-memory-new-plan-red.log`）；最终改为实际生命周期后及成交后维护，新计划保护 green 5.10 秒，实际清仓 P9 prune green 6.45 秒，保留真实 Trade、T+1、费用和精确现金断言。
- G08/G09/G37 测试先编写，但共享模块在途编译错误曾阻止首轮执行；不能把编译阻断报告成其行为 red。初次可执行时 G09 纯 Book 与真实首 root、G37 public step Accepted 与真实取消关联、G08 session 成交日期/订单去重均已 green。
- Node trace parser red：新增 tick 字符串/plan_changes 后旧 parser 拒绝；修复后 5/5 green。DEV 实际 SSR red：记录没有“计划变化”栏目；增加消费后两文件 8/8 green，1.60 秒。
- 所有 Rust 可执行命令使用进程外 `timeout --kill-after=1s 10s`、`--test-threads=8`；后续显式固定 `RAYON_NUM_THREADS=8`，独立 case/套件按互不冲突分片并行。构建使用 `cargo test --no-run -j16 --target-dir .tmp/gap-target` 及共享 `flock /tmp/stock-market-gap-cargo.lock`，不运行全量回归。
- Node 命令使用 `run-with-deadline.mjs 10000`、`--test-timeout=10000`、`--test-concurrency=8`。本 sandbox 默认 fork runner只报子进程失败，改为 `--test-isolation=none` 后获得实际 red/green；未放宽时间上限。
- G08 旧测试 fixture 只有 legacy 持仓初始化，接 dated writer 后显式 NoActiveEntry；改用同一模拟日期的 dated 生命周期准备，保留原全部断言。capture 旧峰值/溢出 fixture 同理改 dated，未改峰值、费用、T+1 或失败守卫期望。
- 最终 `decision_snapshot_capture_tests` 18/18 green，0.54 秒；`account_settlement_tests` 初次 22/24 green，两项 dated fixture 边界修复后 24/24 green，4.47 秒。
- G42 的 9 股拒绝档实际 `GameSession::restore` 两个独立 case 均 green（memory/watchlist 各约 5.7 秒）。原一个 case 做多次完整 restore、以及接受侧完整 9 股 restore 超过 10 秒，均被外部 deadline 终止；改为拆独立拒绝 case，接受側调用 restore 首步的完整生产 `validate_save_slot`。不宣称已验证 9 股完整恢复后续装配/运行；小 fixture 完整个人状态往返另行短验收。

## 独立复核

`review_personal_strategy` 已逐条静态复核并要求补强真实 caller、有限现金、独立 restore 分支和同批新计划保护；有效发现均已修复并再次复核。2026-10-04 最终独立签核确认本组六 G 在限定短测范围内通过，无剩余必须修复代码发现，见 [personal-strategy-review.md](personal-strategy-review.md#最终独立签核2026-10-04)。reviewer 亲跑 Institution/Retail scope 两例 2/2（0.27 秒）与 fundamental 26/26（0.03 秒），并核对完整 diff 及绿色日志。

## 最终固定证据与未验证边界

- 最终 engine source `cargo test --lib --no-run -j16 --features simulation-diagnostics` 编译 green，22.16 秒；固定 `.tmp/personal-final-engine-tests` SHA-256 为 `4940159ec290d40adaeb740cbb7c84a2091b1df978c7635730fbccd69e9a20cf`。固定 `.tmp/personal-final-fundamental-tests` 为 `19dd499319f47058b33e49f2520335e91589f30683dcca268c9ffdc6cb0373d8`，26/26 green。
- 最终日志 `personal-final-*.log` 覆盖 Institution/Retail scope 各 1/1（各 0.27 秒）、capture 18/18、真实受理 trace 1/1（0.80 秒）、真实取消 trace 1/1（0.82 秒）、有限现金新旧计划 1/1（0.32 秒）、个人隔离 1/1（0.32 秒）、四独立 Save map 小 fixture 完整 restore 1/1（0.79 秒）；fundamental 见 `personal-fundamental-final-green.log`。
- Node trace/parser、DEV SSR、personal schema 三文件最终 14/14 green（初次 0.438 秒，最终日志重跑 0.463 秒），见 `personal-final-web-green.log`；不将普通产品快照或 release 传播私有 trace。
- `personal-save_validation_accepts_eight-green.log` 与 `personal-save_validation_protects_active-green.log` 分别 1/1（5.58/5.60 秒）；接受侧覆盖完整生产 SaveValidation guard，不覆盖 9 股完整 restore 后续装配。9 memory/9 watchlist 的实际 restore 拒绝与小 fixture 完整 restore 另有 green。原超时记录保留。
- `personal-personal_root_prefers_known_consolidated-green.log` 的旧记录实际 FAIL：原 fixture 日期已有更新中期，错误预期 Annual 优先；改用 Annual 发布后、Q1 发布前合法 04-10，再次生产测试 green。未弱化 scope 断言，文件名不是结果。
- `personal-fundamental-green.log` 旧记录实际 23/26、三项 FAIL：旧辅助 DCF gold 未同步 G06 逐年折现。root 以独立 JS BigInt/有理数手算计算，`flows.rs` 仅按独立结果将 `10_216_933` 改为 `9_500_000` 并同步中文手算说明；其他方向、增长、信心、材料与字节断言保留。独立数学及动态复核见 [dcf-golden-review.md](dcf-golden-review.md)，不归作 G09 算法改变。
- G07 独立实施及其 72 例测试见 [retail-beliefs.md](retail-beliefs.md)，本组只负责共享 capture/selector 契约与 scope 跨观察例，不冒认其成果。合并报告源头 parent NI 必填守卫由 company assembly owner 实施并单独复核。
- 未运行全量回归、长验收、多年度统计或三宿主完整构建；未 commit、未改 Git index。本签核不能扩大为全仓库验收通过。
