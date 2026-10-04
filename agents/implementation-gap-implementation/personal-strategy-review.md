# 个人策略独立复核

## 范围与门禁

- 非实施者 reviewer：`/root/review_personal_strategy`；作者：`/root/implement_personal_strategy`。
- 目标 worktree：`.worktree/implementation-audit-final`。仅复核 G08、G09、G37、G38、G42、G43 及这些项目依赖的共享 Session / restore 接线；G07 由其他 reviewer 负责。
- 已阅读 AGENTS、principles、open-questions、ADR-0013 / ADR-0016 / ADR-0026、原总账及 reaudit-engine / luna03 / luna05 / luna06 的相关证据。
- 本组改变个人策略游戏假设，不改变 A 股价格优先/时间优先、T+1、整手/零股、费用和价格带；官方交易规则仍以 trading-rules 登记的现行沪深规则为基线。未重新联网核验法源，不宣称新增交易制度实现。
- 当前为中途审查。作者明确表示 G38 的 `|| true` 与 G42 暂缺 `price_memory.prune` 是运行红测前的临时 mutation；不得把此时源码当最终实现签核。

## 逐项中途结论

| 项目 | 已核对生产路径 | 当前门禁 |
| --- | --- | --- |
| G08 | 开局 dated holding、本人观察 dated writer、真实成功结算 dated writer、P2 failure_influence 冻结输入及 P3 消费 | 需真实 GameSession civil_date / trading_day 和 20 交易日衰减短测，不能以机构既有 ADR-0026 衰减或纯读取器测试核销 |
| G09 | Root / Retail 收集本人新获知材料；中期收入累计口径同比更新增长，不把中期盈利当全年估值基数；保留年报及中期 used_report_ids | 当前 pure BeliefCase 验证未获知材料拒绝及增长修订；需真实 Root 中期消费（首次同时获知年报与最新中期）短测与实际 red/green |
| G37 | 实际 PlanRouteOutcome accepted/canceled IDs enrich、plan 状态前后差异、真实 AllocationGrant constraint enrich；DEV feature 隔离 | 当前 cancellation test 手动建立 trace 再驱动生产取消；需生产 Root 建 trace 后真实订单受理关联证据，不能将实际成交与候选或委托混为一谈 |
| G38 | Root 接受时记下 existing_plan_ids；后续同账户分配区分 ExistingPlan / NewOpportunity；卖出归 RiskReduction | 当前 temporary mutation 待红绿收敛；分类不改变跨账户撮合优先，不二次惩罚上游已消费的机构信心 |
| G42 | Root / Retail watchlist 候选先修剪；restore 对持仓 ∪ active plan 之外条目计数而非全市场容量 | price_memory prune 待红绿；需 8 接受 / 9 拒绝以及 active plan 与持仓保护的 restore 断言 |
| G43 | root_candidate_codes 不再从全部历史 belief entries 自动加入；保留本人历史已知材料 | helper 失败证据存在；需本人真实 Root 淡出后不获新材料、不分析、不新建计划的闭环证据，重新发现仍合法 |

## 复核边界

- 只读取作者已运行证据并等待其共享 compile 队列；本 reviewer 当前未运行测试、完整回归、统计验收或宿主构建。
- 已将上述定向缺口发给作者及 root。最终 green 收敛后重新完整检查 diff，再更新逐项结论；当前不宣称完成或通过。

## 跨层待确认：集团材料选择

- `discovery_candidates` 按 PublicationId 升序返回；生产披露先公布 Standalone、再公布同公司 / 期间 / sequence 的 Consolidated 报告。
- Root / Retail 的最新材料选择及 `latest_own_annual` 只比较 `(period, sequence)`，相等时保留先出现的报告。因此同公司集团报告虽进入个人已知信息，实际估值仍稳定选用 Standalone。
- 已报 root 与作者确认集团 NPC 材料选择契约，并建议与 company owner 协同短测。此处不擅自决定集团股本、估值或 scope 优先级；若当前范围要求消费集团最新公开财报，应修复并复核，不能仅凭公开库有 Consolidated 报告核销生产消费。

### 后续确认与增量发现

- root 已明确：同 period 的本人已知 Consolidated 优先，无本人该 period 合并报告时才用 Standalone；同 scope 内按 revision 排序。此为游戏分析选择，不是法规规定投资者必须这样估值；未获知报告不参与选择。
- 已阅读 `report-scope-policy.md`，独立对照其财政部官方 HTML 本地原文的 SHA-256（`3af99caa9c2545431cf207e1039ec5786915f733408446b9a3c7f9639d10a44d`）、CAS 33 第三十一、三十五、四十、四十一条及第五十四条施行日期（2014-07-01）。取证日为 2026-10-04。CAS 33 确定集团现金流合并抵销，不提供归母现金流拆分；Consolidated DCF 缺可靠归属事实时显式 Unavailable 合理，不能硬乘持股比例冒充法定事实。
- 作者已增加 `AnnualFacts.scope`、中央 selector 和 Consolidated DCF guard，DCF 实际行为 red 日志已读；Web reason enum 与所有 consumer / 正式 ADR 同步仍待最终收敛。
- 新增有效发现：`extract_annual_facts` 对 `net_income_to_parent` 无条件 fallback 到集团累计净利，而 `ReportSet::validate` 未要求 Consolidated 报告该 optional 字段存在。已要求区分 Standalone 合法累计净利与 Consolidated 缺归母拆分的显式拒绝/不可用，避免编辑存档或外部已知材料导致集团收益冒充归母。

## G42 跨 phase 复核

- 真实同 minute tie 的新计划测试先出现 red（`personal-memory-new-plan-red.log`，5.31 秒）：Root 冻结 plans 尚不包含本轮 Lifecycle 新建计划，最终修剪可能丢失其价格记忆。
- 最终 Root 修剪已移到 Lifecycle 完成后的 AccountExecution 预算前，按真实 active 计划保护；Root 开始只修剪旧关注，再记录本轮显式发现，不把全部历史 belief 自动恢复资格。
- reviewer 要求移除取消/重构 route resume 中的提前修剪，以免 follow-up 创建新计划之前重现同一问题；作者已移除。新增日终统一修剪已接入 shared Session，发生在公司结算、披露及日终事件之后、observer / 公共日终保存之前。
- 9 条未保护 memory / watchlist 的两个实际 Restore 拒绝短测各约 5.7 秒通过；完整 9 股接受侧多重重建超过 10 秒已安全终止并如实记录。接受侧改测 Restore 首步的完整生产 SaveValidation guard，不宣称验证 9 股恢复后续装配运行；小 fixture 完整个人状态往返另测。尚待最新 green 与内部 quiet checkpoint 边界说明。

## 最新已确认结果

| 项目 | 当前独立结论 | 实际证据与限制 |
| --- | --- | --- |
| G08 | 生产 dated / 派生衰减接线通过 | capture 18/18 green（0.57 秒，作者回报），已读 settlement 24/24 green 日志（4.47 秒）；实际成功 receipt 日期 / order ID / 重复收据不二次写入；P2 冻结 day19 / day20 衰减输入，不删除事实；P3 消费保持冷静期、T+1、费用 |
| G37 | 生产关联及 DEV wire 通过 | actual public step 受理 ID 与 trace 对应、实际取消及 plan change；Web 字符串 tick / plan_changes / strict parser / SSR 对齐，作者回报 3 文件 14/14（0.438 秒）；不伪造成交、预算约束取实际 grant |
| G38 | 同账户有限现金分类通过 | root 捕获既有 PlanId，真实 allocator 在仅够一份 reserve 时先给 ExistingPlan 全额，再给 NewOpportunity 零额与 InsufficientAvailableCash；不更改跨账户撮合，不重复施加个人信心惩罚 |
| G42 | 生产保护及 restore guard 通过 | 已读新 active plan 保留 memory（5.10 秒）、实际清仓 P9 修剪（6.45 秒）、8 未保护 / active9 接受 guard（5.58 / 5.60 秒）green；P9 只按 applied_receipts 收集个人账户，最终 candidate、journal guard 和原子 commit 前修剪，不做每 tick 全账户扫描；9 股完整接受侧后续恢复仍未验证 |
| G43 | 真实 Root 淡出闭环通过 | 已读真实 Root → Lifecycle → commit 测试 green（0.38 秒），未重新发现股票不获新公告、不修改旧 entry、不新建 plan，历史已知信息仍保留 |
| G09 | 新 scope 契约终审仍待收敛 | 首次同时获知 Annual / HalfYear 的 Root 及纯中期同比修订已 green；新增 same-scope、NI / DCF guard 已在 fundamental 26 例中 green。Consolidated priority Root fixture 原日期错误造成实际 FAIL，已改为 Annual 已公布而 Q1 未公布的 04-10，尚待重测；Retail 中央 selector caller 尚待 G07 owner 同步 |

- `personal-fundamental-green.log` 实际为 23 passed / 3 failed，不是全绿；三个旧 DCF 数值 gold 随 G06 算法变化未同步，由 root 协调独立手算，不削弱断言。本 reviewer 不宣称整个 fundamental 套件通过。
- `personal-personal_root_prefers_known_consolidated-green.log` 当前实际为 1 failed；文件名不构成通过证据。只按内容、真实测试出口与源码日期 fixture 判断。
- 本 reviewer 已运行本组 `git diff --check`，未发现 whitespace error；未再占用共享 Cargo，也未运行全量回归或长期统计验收。最终 G09 与组内记录同步后才可签核整组。

## 最终独立签核（2026-10-04）

本段为最终状态；前文中途的“待测 / 未签核”保留为过程证据，不再表示当前阻断。

- G08、G09、G37、G38、G42、G43 在本次限定实现与代表性短测范围内全部通过独立复核，无剩余必须修复的代码发现。作者确认不再增加业务改动，后续只同步实现工作记录及补跑已有短测日志。
- G09 的 Retail 中央 selector consumer 已落盘并完整复查；Institution / Retail 均从本人已知集合选最新期间、同期间 Consolidated、同 scope 最新版本；已经用于 NewMaterial 的报告不重复 λ 修订。新材料修订与更正 direct 分支分开，不扩大 Q11 未授权分发。
- 同 scope 的 Annual / Interim 基数、Consolidated 缺归母 NI 显式拒绝、Consolidated DCF 缺现金流归属明确不可用均已修复并通过负控；盈利倍数 / 权益 ROE 不被无声改成另一模型。ADR-0016 正式节、Rust 类型和 Web strict reason parser 一致，官方材料与游戏选择区分可靠。
- G42 的生产 Root → Lifecycle → AccountExecution 同 minute 新计划保护与实际清仓 → P9 → SaveValidation 两条闭环均 green。P9 只收集实际 applied_receipts 影响的个人账户，在最终 candidate 内修剪，失败不提交权威状态；日终全账户收口补足其他保护集变化。未新增市场配额、共同仓位上限或每 tick 全账户扫描。
- 已读最新 DEV 真实受理 / 取消关联、同账户有限现金竞争、个人状态隔离和四个 Save map 完整 restore 绿色日志；恢复往返 0.79 秒，个人隔离 0.32 秒。不存在用 helper 通过核销生产 caller 的残留断点。

### Reviewer 亲跑

共享 `-j16` compile 完成后，本 reviewer 未重复编译，直接使用作者确认的固定二进制。两个独立命令并行执行，各自配置 `RAYON_NUM_THREADS=8`、`timeout -s KILL 10s`、`--test-threads=8`：

| 固定二进制 / filter | SHA-256 | 实际结果 |
| --- | --- | --- |
| `.tmp/personal-final-engine-tests` / `prefers_known_consolidated_before_standalone_version_sequence` | `4940159ec290d40adaeb740cbb7c84a2091b1df978c7635730fbccd69e9a20cf` | 2/2 green，0.27 秒，覆盖 Institution 与 Retail 生产消费 |
| `.tmp/personal-final-fundamental-tests` / 全部代表性短 fixture | `19dd499319f47058b33e49f2520335e91589f30683dcca268c9ffdc6cb0373d8` | 26/26 green，0.03 秒，包含同 scope 中期、Consolidated NI / CF 不可用、已知材料与金样 |

已读 `personal-fundamental-final-green.log` 与全部 `personal-final-*.log`；旧 23/26 失败及日期 fixture 失败未删除、未改名冒充通过。G06 数值更新由另一非作者 reviewer 以独立有理数手算与亲跑闭环，见 `dcf-golden-review.md`，本报告不重复把该修正归为 G09。

### 三门禁与未验证边界

1. **大 A 语义与依据：通过。** 不改变价格 / 时间优先、T+1、申报单位、收费或价格保护；净利 / 权益归母与集团现金流不混。scope 优先与个人增长模型明确为游戏假设，不冒称 CAS 33 投资估值强制规则。
2. **必要性与最小范围：通过。** 新增 selector、typed Unavailable、记忆保护收口与 DEV parser / view 仅补当前六 G 及其同范围跨层断点；不引入隐藏共同 V、补钱、强制止损、scope 随机配置或新的开放问题实现。
3. **测试 / 跨层 / 复杂度：通过限定短测门禁。** 有效发现均已修复并重新核对。早期 G08 / G09 / G37 的测试虽先落盘，但共享编译阻断使其首次可执行记录为 green；不伪造先前行为 red。G38 / G42 / G43、DEV parser / view 以及 Consolidated DCF 的真实 red 另有明确记录。

本 reviewer 未执行完整回归、长验收、真实多年度统计或三宿主完整构建。9 股接受侧仅有作者执行的生产完整 SaveValidation guard 绿色日志，未把它描述成 9 股完整 restore 后续装配 / 运行通过；9 未保护项实际 Restore 拒绝与小 fixture 完整 restore 另有短测证据。以上限制必须保留到总交付，不能扩大本签核为全仓库验收通过。

## ADR-0016 行文整合及交付记录增量复核

- 已独立审阅 root 重整后的 ADR-0016 完整 diff（5 行新增、1 行替换）：报告选择、同 scope 中期补充、归母事实与现金流方法边界均融入原“个体认识与可选估值”条款；没有保留或新增按日期命名的末尾入口，没有移动无关决策。
- 同期间 Consolidated 优先、本人信息权限、scope 内 revision、同报告不重复修订，与已签核代码及亲跑 scope 生产测试一致。缺同 scope Annual 拒绝补充；不把 Interim 年化或混用范围。
- 文案正确区分两类失败：Consolidated 缺归母 NI 的来源 / 恢复材料拒绝（`validate_parent_income_source` → Closing 登记 / Closing restore / ReportSet 公布与 PublicLibrary restore）；纯分析抽取的 typed 不可用；合并现金流缺归母归属的合法报告，仅使 CashFlow 方法不可用，不冒称整个报告失效或切换估值模型。
- 财政部 CAS 33 官方全文链接完整保留、URL 未变；准则条款与已核验材料一致，明确游戏 scope 选择而非法规投资定价要求。未发现指向被删除末尾日期 heading 的仓内 fragment 引用。
- `personal-strategy.md` 最终增量已完整阅读，已用最新绿色结果替换旧待测状态，并保留旧真实 FAIL、TDD 编译阻断、固定 binary hash、G42 接受侧 guard 与完整恢复未测的区别。独立复核链接和 G06 数学复核链接均指向现存记录，无交付语义漂移。
- 定向 `git diff --check` 通过。本次仅复核文档与记录，没有修改产品、Git index 或执行 commit，也没有为纯文案移动重复运行产品测试。三门禁通过；该 ADR 行文整合与本组六 G 可进入 root 的提交门禁，原完整回归 / 长验收 / 9 股完整接受恢复限制继续保留。
