# sweep11：ADR-0023／0024／0025 逐条实现复核

## 范围与读取记录

- 审查日期：2026-10-03。任务基线为 `b76ece3`；执行只读检查时 worktree HEAD 为 `4ad5a2e`，按主控给定的 merge HEAD 产品代码相同前提检查当前文件。
- 已读取根 `AGENTS.md`、`docs/principles.md`、`docs/open-questions.md`、`docs/architecture.md`，相关目录检索未发现更深层 `AGENTS.md`。
- `docs/decisions/0023-synthetic-history-and-matching-only.md`：61 行，连续读完 1–61。
- `docs/decisions/0024-shrinking-investor-cash-pool.md`：53 行，连续读完 1–53。
- `docs/decisions/0025-day-end-only-persistence.md`：55 行，连续读完 1–55。
- 对照旧总账 `agents/implementation-audit/implementation-audit-2026-10-02.md` 的 R01／S01、G01–G05／G15／G20／G35／G39及已实现反证；只新增本工作记录，未修改产品，未运行测试或长验收。

## ADR-0023：虚拟前史与实际撮合

| 原文位置与条款 | 状态 | 当前代码与结论 |
|---|---|---|
| 15–16：“带 seed 的虚拟日 K”“360 根负时间日 K”“隔离于会话决策 RNG”“trade_stats 为 None” | 已实现 | `packages/engine/src/session/candles.rs:208` 固定 360；`:220` 用 seed、股票哈希与专属盐创建局部 SplitMix64；`:246` 负时间；`:252` 明确 None。前史 volume 是虚拟历史量，不冒充本局逐笔成交。 |
| 17–18：“从第一个游戏交易日开始……实际受理与撮合” | 已实现 | `packages/engine/src/session/pipeline/continuous_tick_finalizer.rs:372` 校验真实成交量额后 `:411` 更新日 K；`packages/engine/src/session/pipeline/auction_day_end.rs:1135` 逐条实际 matches 更新。未发现运行期随机生成日 K 替代撮合链。 |
| 19–21：“day = 0……日 K 时间为 0” | 已实现 | `packages/engine/src/session/candles.rs:139` 用 day×86400；`:192` 取 state.day，不推迟到 day=1。UI 日序另有旧总账展示审查，不要求改变内部 day 定义。 |
| 22–23：“零成交占位 K”“首笔真实成交取代占位 OHLC” | 已实现 | `packages/engine/src/session/candles.rs:140` 零量／零 trade_stats；`:151` 首笔替换整根 OHLC；`:170` 仅正量累计笔数与成交额。`auction_day_end.rs:1131` 无 match 只传零量。内置测试 `candles.rs:363`、`:419` 分别覆盖零成交归档及首笔替换，未运行。 |
| 24–25：“C06……不适用/不在产品范围”“保留……验证” | 政策接线已有；长期证据不能代称通过 | `scripts/simulation/baseline-run.mjs:1391`、`:1416` 输出 not_applicable_synthetic_history_only；`scripts/simulation/verify-simulation-artifacts.mjs:474` 强制该状态。旧总账 G39仍覆盖并发比较契约问题，不能从 policy 标记核销长期验证债。 |
| 26–27：“当前开局不导入真实市场前史” | 当前范围符合 | `candles.rs:212` 从 setup／seed生成；当前未要求外部历史导入。不是新的未实现需求。 |
| 31–39：生成器／实际成交接线、runner v8、旧密封记录不改写 | 已定位现行生产实现；历史执行结论保持边界 | `baseline-run.mjs:62` 与 `verify-simulation-artifacts.mjs:18` 都使用 2026-09-30-synthetic-history-policy-v8。没有改写历史 artifact 或声称旧矩阵已通过新版验收。 |
| 40–47：旧定向验证记录 | 历史证据，不是本轮运行结果 | 本轮没有运行这些测试，不复述为本轮通过。 |
| 49–61：替代方案、后果、关联 | 无新增实现义务 | 不恢复真实行情授权等待，不要求预测拟真或长期有收益。 |

## ADR-0024：允许现金池减少

| 原文位置与条款 | 状态 | 当前代码与结论 |
|---|---|---|
| 20–21：“不要求资金循环……允许税费流出后……减少” | 已有生产语义 | `packages/engine/src/session/pipeline/settlement.rs:54` 只消费真实 Fill；`:161` 累计 receipt.charged 佣金、印花税、过户费；`packages/engine/src/account.rs:332` 买方扣成交额＋费用。税费不是另一投资者收入。 |
| 22–23：“不……新增 NPC 收入……定期补钱、费用返还……不得偷偷制造现金或对手盘” | 未发现违反该范围的接线 | 结算按真实 receipt准备账户 patch（`settlement.rs:95`），没有为现金池恒定添加补钱模块；检索 top-up／cash injection／cash reset／refund fee／daily income等未发现该生产机制。这里只证明已读链路与检索未发现，不能把有限静态检查称绝对不存在任何路径。 |
| 24–25：“不承诺持续成交……零成交是合法结果” | 已实现 | `auction_day_end.rs:1131` 无 match仍维护零量 K；`candles.rs:363` 零成交不进入 traded sample。缩量本身不是新代码缺口。 |
| 26–27：“按实际资金与费用校验……不允许隐性透支……不自动缩小明确申报的数量” | 主干已有 | `account.rs:336` 资金不足显式 InsufficientCash，`:342` 精确扣款；`settlement.rs:119` 按 receipt.qty 结算，错误显式返回。策略事先决定是否报价不等于自动改用户数量。Envelope／费用边界沿用现行 ADR-0017，不能凭这一决策重写既有实收规则。 |
| 28–29：“公司经营……独立记账”“不执行分红、增发、回购……除权除息” | 股东隔离范围已有；经营局部债已覆盖 | `packages/engine/src/session.rs:2105` 操作 company operations／civil clock，随后封账与披露，不把经营利润记入 AccountBook。经营支付缺口已在旧总账 G35，非投资者补钱遗漏；股东行为明确不执行。 |
| 30–31：“Q12……核销”“未来……另有……ADR” | 范围已定 | 不把旧 Q12的资金循环方案计入未实现条目；无需新增现金循环功能。 |
| 9–16、33–44、46–53：背景、替代方案、验证与关联 | 无新增生产实现义务 | 文档明确当批只更新决策，未声称长期经济情景全面验收。 |

## ADR-0025：公共日终保存与加载

| 原文位置与条款 | 状态 | 当前代码与结论 |
|---|---|---|
| 12–14：“只由成功的自然日日结触发……经营……封账和披露……休市日” | 已实现完整接线 | `packages/engine/src/session.rs:2073` 先核实当日交易会话完成，再 checkpoint；`:2105` 经营，`:2110` 封账，`:2114` 披露；`packages/engine/src/session/protocol/civil/session.rs:283` 收完整 CivilUpdate并校验，`:311` 才生成候选，失败 `:317` 回滚，成功 `:328` 替换候选。休市日不要求市场 tick但仍走相同日结链。经营细节 G35不能核销，但不另重复建项。 |
| 15–16：“日内不生成……当前状态存档……点击保存只安排……首个日结前……提示” | 已实现 | `protocol/civil/session.rs:175` 只克隆已完成候选，`:181` 首次日结前显式错误；`apps/web/src/app/useSaveCommands.ts:91` 只提示已启用日終自动存档；`:146` 只选择文件目标；`apps/web/src/save/save-file.ts:251` 旧即时保存出口显式拒绝。 |
| 17–18：“回滚检查点……不是持久存档”“活动委托……不再通过完整存档查询” | 已实现 | `protocol/civil/session.rs:73`、`:79` checkpoint／rollback在内存；`:765` 内置测试确认活动竞价订单在不能save时仍可查。`apps/web/src/host/remote-host.ts:247`、`tauri-host.ts:214` 用独立 playerWorkingOrders查询。没有因内部 GameSession.save存在而登记假缺口。 |
| 19–20：“三宿主共用……不可变候选……异步写入……来自该候选” | 已实现 | `protocol/civil/session.rs:186` 用 seq及 settled_date取候选；`:325` 保留未捕获完成日。`apps/web/src/App.tsx:237` 捕获 CivilUpdate引用，`:240` host.save(reference)并validateDayEndCandidate；`apps/web/src/save/day-end-persistence.ts:27` 先捕获再串行写。日内推进不会使旧引用改成新状态。 |
| 21–22：“文件目标……重复更新……不……一串下载……快速槽……日终” | 已实现 | `App.tsx:234` 仅 CivilUpdate分支写目标；`:243` 快速槽；`save-file.ts:137` 闭包持有选定 handle，每次创建 writable覆盖；`:53` Tauri目标持有选定 path；`:263` 不支持覆盖明确报错，不下载。 |
| 23–25：“失败……显示原因并保留上一份……旧异步响应不得覆盖……T+1、费用与披露” | 主要已实现；宿主既有缺口仍成立 | `apps/web/src/save/save-repository.ts:72` 压缩完再检查generation并setItem；`save-file.ts:155` browser临时 stream写后close，失败abort；`:68` Tauri独占临时文件，`:80` rename替换，失败显式清理错误。`day-end-targets.ts:8` 各目标独立allSettled，汇报部分成功／失败；`App.tsx:247` 只在写入返回后报成功。`useSaveCommands.ts:107`、`:173`、`:221` 替换前invalidate并等待idle。资金／T+1／费用仍由 Account／SaveSlot权威恢复，没通过保存流程改账。Remote暂停／继续旧baseline为已有G04，不重复登记。 |
| 26–29：“只读取一次快速槽……先……配置和种子恢复……新局不加载旧档……坏档显式报错” | 已实现；普通新局取种旧债G20 | `apps/web/src/save/session-replacement.ts:5` memoize唯一pending读，`:11` 消费；`:21` reset标为已消费。`useSessionHostLifecycle.ts:90` 单读入口，`:96` 存档setup，`:97` 存档seed，`:105` 在host.start前load；`:181` 失败显式初始化错误。后续repository.load仅明确handleLoad（`useSaveCommands.ts:103`）。DEFAULT_SEED新局问题已为G20。 |
| 30–33：“公共加载……完整日结档”“不接受……活动委托……冻结……待受理请求”“跨日个人计划……随机状态仍须保留” | 已实现严格公共入口 | `protocol/civil/session.rs:100` 深度GameSession恢复后 `:111` 校验settled/tick/已完成会话；`:119` 拒resting／auction orders、live_envelopes、lifecycles、parent_orders、pending requests。`:147` verification-only恢复明确不产生公共save。`apps/web/src/save/day-end-candidate.ts:6` 前端日级过滤；最终三宿主均调用ProtocolSession::restore，不只依赖浅校验。跨日plans／personal状态仍在现行SaveSlot，不将parent_orders日内执行实体与跨日个人计划混为同一字段。 |
| 35–36：“不合法的日级事实……拒绝……不……修改资产” | 已实现现行守卫 | ProtocolSession::restore先调用GameSession::restore，失败不替换宿主会话；修改日终现金本身不是强行要求支撑已不存在的日内买单。 |
| 38–43：实施范围、精简不丢计划／个人／RNG／历史、严格新格式 | 已有实现与当前范围 | `packages/engine/src/session/persistence/v2.rs:97`／`:109` 严格schema_version，legacy明确拒绝；同文件各事实结构deny_unknown_fields。不新增旧格式迁移、真实行情或股东资金流。 |
| 45–55：代表性短测及后续记录 | 历史验证边界 | 已读候选冻结、失败日结、拒日内加载测试（`protocol/civil/session.rs:535`、`:555`、`:804`），以及`day-end-persistence.test.ts`、`file-target.test.ts`、`day-end-targets.test.ts`；均未运行。不能称浏览器／三宿主完整矩阵通过。 |

## 三宿主实际入口与错误反证

1. WASM：`apps/web-wasm/src/lib.rs:483` 调ProtocolSession.save；`:492` save_candidate；`:502` 与`:510` restore均经registry，registry的`:70` 用ProtocolSession::restore。`apps/web/src/host/wasm-worker.ts:200` save先requireGeneration，候选指定时不降级到当前save；不存在binding显式错误。
2. Server：`apps/server/src/actor.rs:1342` 保存校验timeline_generation与fatal；候选明确用save_candidate；`:1358` restore用ProtocolSession且先成功后换会话。`apps/web/src/host/remote-host.ts:233` 请求携generation与candidate，`:240` 拒旧响应。
3. Desktop：`apps/desktop/src-tauri/src/actor.rs:1027` save校验generation并走相同候选；`:1139` restore校验generation、调用ProtocolSession::restore。`apps/web/src/host/tauri-host.ts:205` 请求／响应都检查generation。
4. 错误不会被队列维护用的`tail = operation.then(..., ...)`吞掉：`apps/web/src/save/day-end-persistence.ts:36` 返回的是原operation，App的`:249` 接收拒绝原因。tail只让下一日可继续保存。
5. 快速槽配额不足不阻止授权文件输出：`day-end-targets.ts:8` 并行尝试全部目标；`:21` AggregateError含各目标状态。它没有要求多目标分布式原子回滚，不能因某目标成功而另一目标失败另造合同。

## 新候选与穷尽边界

**本分工没有确认旧总账之外的新代码遗漏。** 旧总账R01／S01关于合成前史、真实成交、允许现金减少、公共日终候选和公共日级加载隔离的概括，经本轮逐条追链仍有生产代码支持。

已排除的假阳性：恢复真实行情导入／校准；为长期缩量补钱或造对手盘；把GameSession内部存档投影／verification checkpoint视作日内用户持久保存；把异步写完发生在盘中视作捕获了盘中状态；把日内parent_orders删除误认为删除跨日个人计划；把queue.tail拒绝归一误认为UI静默吞写入错误。

有限静态核对不能证明所有浏览器文件系统、权限撤销、跨进程rename故障或三宿主长运行矩阵已验收。候选保存／读取代码已实现与完整部署验收通过是两个结论。本报告不以“未运行”制造新产品代码缺口，也不核销旧G01–G05、G15、G20、G35或G39。
