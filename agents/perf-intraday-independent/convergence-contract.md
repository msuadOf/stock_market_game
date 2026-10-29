# 日内独立运行 / 日终汇合：实现合同（任务 #122）

> 状态：**设计合同 v2（未实现）**。本文只定义实现必须满足的合同与验收矩阵，不声称任何性能结论，
> 也不代表代码已落地。实施前置：#121 最终修订须先集成并经独立复核，覆盖日界后
> `public_views` 原子刷新、`AbortDayEnd` 参与者撤销、下一 `Start` 部分发送后的不可撤边界
> 与 `dead_reason` 终止。旧轮次的复核不代表这些最新边界已在主区落地。
>
> 术语与代码对齐：`World` / `RuntimeDriver` / `GameSession` / `PublishedTick` /
> `ViewPublication` / `DeskShard` / `GroupShard` / `RetainedIndex` / `SessionSetup`。

---

## 2026-10-29 受限修订：更正 prepare 业务拒绝

本修订明确改变原 v2 的一处失败政策，承接随后已批准 Host 的取消更正与 civil-only 重试方向；
不回填旧源码准字、原失败或实际运行结果。市场 Full swap 仍不可撤销。仅当真实更正 prepare
返回 typed `ReportCorrectionDayEnd`，分类显式为 `InvalidInput` 或 `BusinessCondition`，civil
候选尚未提交且尚无首个 fatal，才保留已安装市场、retained 和旧存档候选，允许取消坏更正后
仅重试 civil。不得额外发送市场 `Start`、重跑收盘或恢复旧 checkpoint。

例外必须由真实 prepare 调用点的来源标记证明，不能只在整个 candidate 的错误出口按类型判断；
输出 callback 即使返回同型错误也仍 terminal。`UnsupportedOperation`、`SystemState`、INFO、
内部不变量、其它 civil／输出失败以及已提交 civil 后的失败仍执行原 F2/F3。禁止用
`!= SystemState` 自动豁免其它或未来分类。以下 §1.2、§2 #11/#12、§4.2 的 terminal
规定均仅扣除此受限例外，其余不变；不同实现或错误阶段不能借用该例外。

未安装实际市场收盘 Full 的首个 Closed 日保留原有可逆 callback 拒绝，不新增按错误外形终止的政策；
SystemState 或已 terminal 的 World 仍按原健康门封闭，不得由本例外复活。

## 修订记录 v2（逐条响应 Sol / Luna 退回意见，当前文本待独立终审）

| 编号 | 原问题 | 处理位置 | 处理方式 |
|---|---|---|---|
| S1 | 三档未钉死同一汇合实现；账户有稀疏/全量两条路径（`drive.rs:716-772`） | §1.2、§4.1、§6 #7/#13 | 定义唯一入口 `flush_candidate`；账户来源由**领域原因**（`account_source_reason()`）决定，与策略无关；三档只决定调用时机。新增同一次运行内的等价性用例与失败等价用例。 |
| S2 | §2「civil 日结失败」与真实回滚域冲突 | §2 第 11、12 行，续见 R1 | 保留旧轮次查得的 `drive.rs:1247-1275,1701-1713` 与 `actor.rs:1923-1928,2280-2287` 依据；该旧撤销只适用于当时无完整安装的顺序，R1 明确新合同不在安装后回退 checkpoint。已发布 tick 不回退的前提不变。 |
| S3 | civil 权威方向矛盾（代码为 session 推入 World） | §1.0、§1.1、§5 | 显式声明 civil date 为 **session 权威**，`drive.rs:624`、`drive.rs:1261` 单向推入 World；新增 flush 前置断言与顺序。 |
| S4 + L4 | 性能门无可判定阈值 | §6 #17（移除）、§7 | 本轮**不设性能门**，只产出配对测量数据；翻转默认必须另开决策门并取得用户批准。 |
| L1 | 三档等价用例要求跨运行逐字段相等（并发下必 flaky） | §6 #7、#8 | #7 改为**同一次运行内** World 权威态 vs 镜像逐字段比对；跨档对照只允许确定性 fixture，并声明其不代表自由并发契约。 |
| L2 | 失败矩阵与 #121 冲突 | §2、§2「不可违反项」 | 与 S2 合并；增加红线：任何路径不得撤销已发布 tick。 |
| L3 | OnDemand 何时汇合存在两条互斥规则 | §4.3、§4.6、§6 #9/#10、§8-2 | 唯一规则：仅在显式 flush API 与**计算消费点**同步 flush；诊断读面不 flush。DayEnd 的 ingress 行为定为**显式拒绝**（不再二选一）。 |
| P2-1（Sol） | OnDemand 读面集合未分类 | §4.6 | 按运行账户消费、必需日结消费、显式同步、只读候选四类列准入表，另列诊断读面。 |
| P2-2（Sol） | dirty 累积存放/幂等/上界/清理未定义 | §1.2-C | 定义 driver 私有覆盖表、稳定键、序号、幂等、可判定上界与清理；事实流不进入覆盖表，并加用例 #13。 |
| P2-3（Sol） | 稀疏与全量完整性校验混写 | §1.1 账户行、§1.2-B | 分路径写：稀疏→`ensure_fact_owners_covered_arc`；全量→名册长度/ID 集合校验。 |
| P2-1（Luna） | 缺少 dirty/事件/generation/save/旁路用例 | §6 #13–#16 | 新增 4 例，全部给精确断言。 |
| P2-2（Luna） | 用例 9/10/11 断言弱 | §6 #10–#12 | 写死 fixture 数值、精确错误枚举与金额锚。 |
| P2-3（Luna） | 测试资源规范未承接 | §6「资源分类」 | 普通用例 ≤10s；配对测量登记为长验收（300000ms，299000 执行 + 1000 收尾，进程外监督，独占整机）。 |
| P2-4（Luna） | 内存只有记录没有判定 | §1.2-C、§7 | 仅覆盖表有固定名册下的条目数上界；append-only 事实、计划历史和总 RSS 分开记录与归因，不声称全部内存与 tick 数无关。 |
| R1（2026-10-25 原始代码复核） | civil 前同步安装与旧 `rollback_day_end` 仅撤 day/seq 的范围冲突 | §1.2-A、§2、§3、§6 #11/#12/#18 | candidate 安装前失败不改镜像；安装成功不可撤。随后 civil 失败终止，不调用旧撤销或用 checkpoint 复活旧视图。此为待实现边界，不冒称现代码已支持。 |
| R2（2026-10-25 原始代码复核） | attention/plans 含 seq 的键随 tick 增长；旧 RSS 承诺涵盖了必须保留的事实 | §1.1、§1.2-C/D、§6 #13/#14、§7 | attention 改稳定账户键；plans 在 flush 用既有无门控 `collect_plans()` 整簿读取，不累积计划事件。保留交易/计划事实的交付与历史责任。 |
| R3（2026-10-25 原始代码复核） | DayEnd 禁止全部计算消费点，误禁公司链与存档 | §4.2–4.6、§6 #10/#11 | 禁令只针对日内玩家 ingress 及其资源校验；三档都执行日结所需同步 flush，公司链和日终候选生成照常准入。 |
| R4（2026-10-25 原始代码复核） | 旧候选读取、当前日请求与 save 自动 flush 混写 | §2 #13、§3 API 决策表、§4.3/4.6、§6 #5/#16 | 保持现有只读候选 API 不 flush；3018 只用于显式指定日期的当前日请求。宿主日内保存按 ADR-0025 安排日终，不能把旧档冒充当前状态。 |

> 原 v1 的问题描述与设计意图保留在各节；本次只改冲突表述并补齐可执行条款。

---

## 0. 目标与非目标

**目标**：让引擎日内（非日界拍）不承担外部会话镜像安装成本，同时不改变任何 A 股交易语义、
World 市场提交边界与资金所有权语义；日终一次汇合，且汇合时机可配置。

**非目标**：
- 不取消 `World` 内部 Start/收敛/Seal/SealAck，不改变“提交点=发布点=Abort 窗口关闭”。
- 不取消每拍 `PublishedTick` 发布。
- 不把交互宿主（Web/远程）自动降级为滞后行情。
- 不引入通用事件溯源重写；只推迟 session 镜像安装与派生诊断面。

**独立身份硬前置（未交付；阻止开启 #122 实现）**：
- 先独立交付计划身份/归属修复并取得未实施者的可信独立复核。无论采用何方案，必须保证
  PlanId 全局稳定唯一、owner 可验证、每账户单 writer、session/Player 与组的权威归属明确，
  保存恢复及分组拓扑变化后原引用不变、持续分配不碰撞。
- 必须取得真实短验收：至少两组各自创建首个计划，完整 driver flush 成功并包含双方全部计划；
  同时覆盖跨分组保存恢复后的继续分配与引用一致性。完整身份 diff 与验收证据须可信独立审查通过。
- 未交付此前置时，本文即使获条件通过也不能开启 #122 实现；不得用碰撞 terminal、降为单组、
  任意容量上限或静默 remap 冒充正常多组支持。#121 的实施前置仍独立保留。
- [独立身份方案](plan-id-ownership-design-2026-10-25.md) 推荐 owner+sequence，但仍待独立审查；
  本合同不据此默许全部类型/存档改造已经完成或直接放行实施。

**硬前置（来自 Sol 裁决第 1、6 项）**：
- 判据：**下一拍决策、资源校验或日终领域迁移会读取的数据不得仅因位于 session 就推迟。**
  NPC group/desk/协调者/Start/收敛/Seal 属引擎内部，不参与“可推迟”讨论。
- 交互宿主存在日内玩家委托入口（`pending_player`/ingress），其可用资金、持仓、
  可卖数量来自 session 账户读面，属**计算读面**；因此 `DayEnd` 只允许用于
  **无日内玩家 ingress** 的无头批量仿真。交互宿主默认 `EveryTick`；选择 `OnDemand` 时，
  运行账户消费点在读取前同步 flush，必需日结另有三档共同屏障（规则见 §4.3/§4.6）。

---

## 1. 汇合 candidate 的权威读取清单

### 1.0 权威来源与方向（v2 修订，闭合 S3）

- **市场/账户/计划/attention/行情/K 的权威是 `World`**：汇合输入只能是 `World` 在当前
  `committed_tick` 上的权威态，或自上次成功汇合以来由组/部上行的增量事实。
- **civil date 的权威是 `GameSession`**，不是 World。代码方向是单向推入：
  `RuntimeDriver::step` 在运行前 `world.set_civil_date(session.civil_date())`
  （`packages/engine/src/runtime/drive.rs:624`）；`day_end` 成功推进自然日后再次
  `world.set_civil_date(session.civil_date())`（`drive.rs:1261`）。
- **汇合前置断言**：构建 candidate 前必须校验
  `world.civil_date() == session.civil_date()`；不相等时显式失败
  （`ProtocolInvariant`/`String` 错误），不得按 World 的 civil 继续。
- **顺序约束**：只允许在 `set_civil_date` 之后 flush；日结路径必须在 `end_civil_day`
  成功并同步 civil 之后，才允许生成存档候选。

### 1.1 逐字段来源与版本校验

| candidate 面 | session 目标域 | 权威来源 | 读取方式 | 版本/完整性校验 |
|---|---|---|---|---|
| 进度游标（不随可选镜像延迟） | `state.tick` / `state.day` / 事件游标；civil 见 §1.0 | tick/day：`World.global`；事件 seq：既有 driver/protocol 事件流（`events_installed` 不是公开 seq）；civil：`session.civil_date()` 单向推入 World | 逐提交更新 + §1.0 前置断言 | `published.tick == world.committed_tick`；session 运行 tick 与 World 锁步；mirror 来源游标单独记录，不用最新 tick 包装旧账户 |
| 账户现金/持仓/T+1/成本基 | `state.accounts` | 见 §1.2-B：稀疏分片 `published.groups[g].accounts` 或全量 `World.snapshot_accounts()`，由 `account_source_reason()` 唯一决定 | 全量枚举或累积分片 | **分路径校验**：稀疏→`ensure_fact_owners_covered_arc`；全量→名册长度与 `AccountId` 集合完全一致；两条路径都禁止缺员/重复 |
| 个人态（belief：reference/peak equity、information） | `state.personal`（`set_runtime_personal_state`） | `published.groups[g].belief` 稀疏条目（自上次 flush 累积） | 累积 dirty 表；flush 时按账号整态安装 | 每条 `(group, local)` 唯一；未知账号显式失败；不得用空值覆盖未 dirty 账号；覆盖集必须包含窗口内全部变更 |
| 计划簿 | `state.plans` | `World.collect_plans()`（组端 `plan_book` 权威，`EchoPlansFull` 无 dirty 门控） | 每次 flush 读取各组完整现态并按既有 `PlanId`/归属合并 | 组数/ID/归属完整且唯一；不能把 `collect_plans_echo()` 的空簿当“未变化”。组权威范围内的真实空簿须清除旧镜像；session 自有测试注入/玩家计划须按明确归属保留，不得靠“取第一份非空簿”猜测 |
| attention 调度镜像 | `state.npc_attention` | 组端 `next_due` 权威；`EchoAttentionSparse` 上行 | 累积稀疏条目 | 条目必须属于本组名册且唯一；序号单调 |
| 行情公共视图 | `state.markets`（`StockPublic` 投影） | `World.public_views()`（#121 后日结后为次日新基准） | `install_market_views` 全量 vec | 每股唯一；日界后 `last_close`/涨跌停/笼子必须为新基准 |
| 价格历史/分钟收盘/分钟桶 | `state.price_history` / `market_minute_closes` / `market_minute_bars` | `StockPublic` 的 `recent`/`minute`/`minute_bars` | 逐股合并 | 日界清空与 `roll_day` 对齐 |
| 活动 K / 历史日 K | `state.candle_book` | `World` 发布版本 `desks[i].candle` + `RetainedIndex` | `install_candles` + 日结 `replace_runtime_committed_daily_candles` | 日界 `commit_active_daily_candles` 前必须先装活动 K；终 K 以 Drain 应答补真 |
| 在途委托快照 / 信封 | `state.runtime_resting_snapshot` / `runtime_resting_envelope` 派生 | 普通拍：发布版本 Arc；日结静点：`collect_resting_facts_live()` 与同次 desk 游标 | 普通拍借用；日结读真实清簿后静点，不借清簿前发布分片 | tick 与阶段同版；日结空簿及已消耗 serial 必须真实保留；序号单调 |
| desk 簿镜像 | `state.desk_book_mirror` | 同上快照按 `owner` 归户 | 全量重建或增量差分 | 归户必须用 `(group, local)` 全域身份，禁止只用 local |
| 零售诊断 | `state.last_retail_order_events` / `last_retail_decisions` | 组端/desk 本拍 trace（`collect_retail_traces`） | 本拍瞬态 | 纯诊断，可配置关闭；不得影响交易语义 |
| 保留历史链 | 查询/存档读面 | `published.retained`（`Arc<RetainedIndex>`） | Arc 借用零拷贝 | 日结冻结后原子换新；平日复用 |
| 事件流 | 交付给宿主 | 本拍 `desk_trades` / `desk_lifecycle` / `desk_receipts` / `desk_auction` / `group_rejections` | **每拍组装与交付**（不推迟） | seq 由驱动器游标单调分配；顺序见 §5 |
| 提交态哈希 | 诊断/验收 | `published.hashes`（groups/desks） | 直接读 | 用于跨日校验与回放对照 |

### 1.2 统一汇合入口与稀疏分片规则（v2 修订，闭合 S1 / P2-2 / P2-3）

#### A. 唯一入口与单一实现

三档必须调用**同一个**函数，策略只决定调用时机：

```text
fn flush_candidate(&mut self, session: &mut GameSession,
                   trigger: FlushTrigger) -> Result<(), FlushError>
```

- `FlushTrigger ∈ { EveryTick, DayEnd, OnDemand, ComputeRead }`；`trigger` 只用于
  日志/断言，**不得**改变构建内容。
- 账户来源由**领域原因**唯一决定，禁止按策略分支选择：

```text
fn account_source_reason(&self) -> AccountSource {
    if self.force_full_accounts
        || self.domain_mutated_without_facts_since_last_flush
        || !self.has_successful_flush_baseline
    { AccountSource::Full } else { AccountSource::SparseAccumulated }
}
```

  - `domain_mutated_without_facts_since_last_flush` 覆盖：日结 Drain/T+1 解锁、
    AdminBatch 权益处理、除权除息/配股、公司权益处理等**不产生逐账户 `FillFact`** 的权威变更。
  - 首拍无基线（`drive.rs:558` 的初始化语义）必须走 `Full`。
  - 其余拍走 `SparseAccumulated`（`published.groups[g].accounts` 的累积 dirty 表）。
  - `force_full_accounts` 只能在**同一次同步 flush 内**被消费并清除；实现批必须取消它
    “置位后等下一 step 才回写”的跨拍残留语义（`drive.rs:1268` 置位、`drive.rs:866` 清除）。
- 两条来源共用同一套后续代码：私有 candidate 组装、稀疏域合并、完整性校验、单点 swap。
  **禁止**为三档各写一份安装逻辑。
- **最小 candidate 与安装边界**：只准备本次待替换镜像域、未变域的共享句柄及来源版本；
  不复制无关的全会话历史或新增第二份权威计算。本次镜像及普通拍投影所需的可失败读取、
  映射、校验与输出准备均在私有 candidate 内完成；后续 civil 是独立 candidate。
  单点 swap 仅移动已验证值，之后不再有可失败的安装动作。
  swap 前失败丢弃 candidate，session 和已交付事件不动；swap 成功则该安装**不可撤销**。
  后续 frame/civil/存档准备失败不得调用 `rollback_runtime_step`、`rollback_day_end` 或
  旧 checkpoint 替换已安装版本，按 §2 显式终止；仅本页 2026-10-29 的真实更正 prepare 业务拒绝例外可保留 civil-only 重试。
  现有安装后再回滚的代码须在实现批改造。
- **来源身份**：普通拍用 `(committed_tick, generation)`；日结附加当次
  `DayEndCandidate` 身份/阶段。#121 的次日 `public_views` 是日结广播基，不等于另发一个
  混合账户/事件版本的 `PublishedTick`。日结后的全账户、行情、终 K、resting 必须全部
  来自同一次日结，不能仅凭普通发布槽的同 tick/generation 声称它们同版。
- 等价性证明（实现批必须补的用例，见 §6 #7）：
  1. 同一 tick 流、同一 tick T，分别执行“正常来源”与“强制 `Full`”两次 flush，
     session 镜像各域逐字段相等；
  2. 在 swap 前注入同一失败，两份私有 candidate 丢弃后，镜像、来源版本和覆盖表逐字段相等。
  两条分支从同一固定 World 读点与相同安装前镜像独立构建，不把第二次读取已清空覆盖表
  的 no-op 当作等价证明；不重跑另一条自由并发 tick 流。
  满足这两条即视为“稀疏是经证明等价的优化”，否则必须删除稀疏路径改用全量。

#### B. 完整性校验分路径

- `AccountSource::SparseAccumulated`：
  - 分片必须覆盖本拍全部事实 owner（`ensure_fact_owners_covered_arc`，`drive.rs:49` 定义、
    `drive.rs:741` 调用）；
  - 累积表覆盖集必须 ⊇ 本窗口全部 dirty 条目；缺项显式失败，不静默沿用旧值。
- `AccountSource::Full`：
  - `snapshot_accounts()` 返回的组数、每组 local 数与 `id_map` 名册完全一致；
  - 不得调用 `ensure_fact_owners_covered_arc`（它只适用于稀疏路径）；
  - 任意 `AccountId` 缺失/重复/越界 → 显式失败。

#### C. `DirtyAccumulator`（三档共用，DayEnd/OnDemand 必需）

- **存放位置**：`RuntimeDriver` 私有字段，不属于 `GameSession`；不得通过任何 session
  读面观察到（验收时断言读面不可见）。
- **键与值**：
  - 账户、belief、attention 各一张表，稳定键均为 `(group, local)`；`source_seq` 放值中，
    不能放进键。账户/belief 保存最新整态；attention 保存最新 `(next_due, rng_state)`。
  - plans 不存内容或事件列表；仅一个 `plans_pending` 标记。每次 World commit 将其置位，
    flush 必须用 `collect_plans()` 读取完整权威现态（包括 terminal 记录），无需每拍 EchoPlans。
    不新增计划增量协议、删除墓碑或历史裁剪。测试中的“删除”指从活动索引退出，
    terminal 记录依既有 PlanBook 保留；真实权威空簿也必须与未变化的 Echo 空簿区分。
  - `source_seq` 是 driver 对已提交输入分配的单调覆盖游标，不额外消耗公开事件 `seq`；
    重投同一事实使用原游标，不能每次分配新值冒充不同事实。
- **幂等与乱序**：
  - 同键、同 `source_seq`、同内容 → no-op；
  - 同键、同 `source_seq`、不同内容 → 显式 `ProtocolInvariant` 失败；
  - 同键、更旧 `source_seq` → 显式 `ProtocolInvariant` 失败；
  - 同键、更新 `source_seq` → 替换（只保留最新一条）。
- **覆盖表上界**：名册账户数为 A，则三张表各 ≤ A 条，合计 ≤ `3A` 条，加一个 plans 标记；
  固定名册下与 tick 数无关。此界不包括权威 PlanBook、持仓内容和必须保留的历史事实，
  不承诺这些值的字节量或整个进程 RSS 为常数。
- **清理**：成功 swap 后原子清空覆盖表并复位 plans 标记；失败保留供诊断，随后终止，
  **不允许原会话重试**。驱动器销毁时释放；恢复只走显式读档/新会话，不把旧累积回灌。
- **断言**：用例 #13 锁定条目数与幂等/乱序行为；§7 记录内存随 tick 数的斜率。

#### D. 事件与 `PublishedTick`

- **事件不参与累积**：发布槽只持最新一代；事件必须每拍组装与交付（见 §5 与用例 #14）。
- 既有交易/收据/行情事件按既有身份和保留范围 append-only，不能用最后镜像态替代中间事实。
  driver 在覆盖前须将每拍公开事实完整移交现有事件/历史通道，宿主按现有游标消费；
  未完成移交的事实不得删除。交付失败显式终止，不能静默覆盖或跳号。
- `PlanEvent` 是 `PlanBook` 状态机输入（`plans/mod.rs:84,517`），当前公共 `Event` 没有
  计划变更变体（`session.rs:313`）。它仍由 World 按原 tick/轮次完整处理；不得因只保留
  plans 标记而吞掉尚未执行的建立、修改、成交推进或终结。完整现态/terminal 记录照旧保留；
  不声称存在一条现成的公共计划事件历史，也不新增全部内部 `PlanEvent` 永久归档协议。
- 公开事实与 PlanBook terminal 记录的生命周期沿用 ADR-0017/0025/0034；日内不因此新增
  持久化或 WAL。外存归档仅由现有宿主存储层在成功自然日日结后执行，engine 不做 I/O。
  覆盖表的 `3A` 上界不能转化为事实保留上限或丢弃策略。
- `PublishedTick` 不由 accumulator 重建；镜像按其真实来源版本及日结阶段安装。

---

## 2. 失败边界矩阵（v2 修订，闭合 S2 / L2）

三层边界（Sol 裁决第 2 项）：**普通 tick** / **镜像 candidate 安装** / **World 日结**。
World 普通 tick 的发布不可撤；镜像只允许在安装前丢弃 candidate；World 日结只允许在
#121 的有效 Abort 窗口内撤销本次日结，且不得撤销已安装镜像所承认的日结版本。
下一 `Start` 开始发送即按 #121 关闭日结撤销入口，部分发送失败也不能撤销已转正参与者。

**现状与目标的区别**：当前 `drive.rs:1210-1275` 在 civil 前没有本合同要求的完整 flush；
`rollback_day_end`（`drive.rs:1701-1713`）只恢复会话 day/seq、调用 World Abort、截断 driver
缓冲，不能撤回新增的完整安装。当前 `actor.rs:1923-1928,2280-2287` 的 civil 失败 checkpoint
回滚也不能直接用于本合同的安装后路径。以下为待实现合同，明确选择安装成功后终止而不复活旧态，
不新增整天撤销基线。#121 的最新代码与终审尚须先集成，不能把旧实现当作该前提已经满足。

| # | 场景 | 发生位置 | 已提交状态 | 回滚范围 | 镜像/存档 | 是否 poison | 可否重试 |
|---|---|---|---|---|---|---|---|
| 1 | 普通 tick `World` 失败 | `World.run_ticks(1)` | 未提交本拍；更早 tick 已发布 | 只在本拍有效窗口内按 World `Abort` 撤未提交工作；#121 已关闭的日结窗口不得重开 | 保持上一成功镜像/候选 | 是 | 否；只能显式读档/新会话 |
| 2 | 普通 tick candidate 失败 | 唯一 `flush_candidate` 的 swap 前 | World 本拍**已提交并发布** | 丢弃私有 candidate，不动 World，不改上一镜像 | 上一成功安装版本与旧候选不变 | 是 | 否 |
| 3 | `step_frame`/commit evidence 投影失败 | 必须移到 swap 前准备完成 | World 已提交 | swap 前同 #2；若仍有安装后失败则保留已安装版本并按 #11 终止，不再 discard driver 后重试 | 不复活旧镜像 | 是 | 否 |
| 4 | 日结 Drain 发送失败（部分 desk 已收） | `World.run_day_end_inner` ① | 当日 tick 已逐拍提交并发布 | 仅向**已收到 Drain 的 desk** 发 `AbortDayEnd`；未收到 `DayEnd` 的组**不得**回滚普通日志（#121） | 保持上一成功发布基 | 是 | 否 |
| 5 | 日结 Drain 收集失败（收到 k 个后） | 同上 | 同上 | 同 #4；仅本次未提交日结的 `drained_*` 缓冲清空 | 同上 | 是 | 否 |
| 6 | 组 `DayEnd` 发送失败（部分组已收） | 同上 ② | 同上 | 只撤销**已收到 `DayEnd` 的组**（`rollback_day_end` 到 Seal 基线）；未收到的组保持 240 | 同上 | 是 | 否 |
| 7 | 组 `DayEnd` 处理失败 | `GroupState.handle_day_end` | 同上 | 该组整体冲回；World 按已发参与者集合撤销 | 同上 | 是 | 否 |
| 8 | World 日结候选校验失败 | 镜像 swap 与下一 Start 之前 | 同上 | #121 参与者撤销：仅撤本次日结；candidate 丢弃，`public_views` 恢复真实 prior，普通发布版本不动 | 上一成功安装版本 | 是 | 否；底层健康注入测试可验证 Abort 的机械可逆性，不授权生产恢复 |
| 9 | 日终**汇合 candidate 校验失败**（三档） | `flush_candidate` 构建/校验 | World 已提交当日 tick | 丢弃 candidate，覆盖表保留；只在日结尚未安装、相关保留事实尚未发布且 #121 窗口仍有效时撤本次日结。若已有日结事实发布，保留该版本并终止，不能回退发布或历史 | 上一成功安装版本与旧候选不变；正常读面封闭 | 是 | 否 |
| 10 | 汇合安装准备失败 | 单点 swap 之前 | 同上 | 同 #9；swap 只移动已验证值，不设“成功 swap 后撤安装”的分支 | 同上 | 是 | 否 |
| 11 | 已安装日结镜像后 civil/输出准备失败（受限更正 prepare 例外见本页修订） | 同步 flush 成功后 | 普通 tick 已发布，市场日结镜像已完整安装；civil 独立 candidate 尚未提交 | **不调用** `rollback_day_end`/`abort_day_end`，不恢复 day/账户/T+1/行情/K/mirror/resting 或覆盖表；丢弃失败的 civil 私有 candidate。市场镜像保留该日结阶段，civil date/已交付 seq 保持各自最后成功值；不发虚假 civil 成功事件 | 保留已安装市场版本，旧存档候选内容不变；不生成新候选；正常读面封闭 | 是（driver/session 同步记 terminal 首错，宿主 fatal、停推进、关 ingress） | 否；同一会话不得 flush/step/day_end/撤销或重建 driver 后续跑 |
| 12 | 宿主批 civil 准备失败（仅传播已证明的受限更正 prepare 拒绝可例外） | `prepare_civil_updates` 的新接线 | 本批可能未广播，但内部 World/镜像已有不可撤提交 | 按 #11 保留并终止；**不得**因未广播而 `ProtocolSession::rollback(checkpoint)` 复活旧视图或回退内部发布版本。丢弃未提交 civil 输出/可重建传输投影，保留已提交市场事实与游标，显式报告本批未交付范围；先前交付不动 | 旧候选保留但普通 save 受健康门拒绝 | 是 | 否 |
| 13 | 读取候选/显式请求当前日存档不满足条件 | §3 决策表 | 不改变任何市场状态 | 不 flush、不自动日结、不回滚；依请求意图返回 3015/3016/3017/3018，terminal 错误优先 | 不能把旧候选冒充当前状态 | 否（健康会话的调用错误） | 可在自然日成功完成后重发只读请求；单独 flush 不足以生成存档 |
| 14 | 日终候选成功后宿主写入失败 | 现有存储适配层 | 市场/civil 与候选均已成功 | 不回滚交易或候选；显式报告写入失败，保留上一有效持久档 | 可按精确 key 重试同一不可变候选；不能转而读取盘中新态 | 否（存储故障） | 按 ADR-0025 的写入流程显式重试 |

**不可违反项**：
- 任何失败都不得留下半安装（行情已写、账户未写，或反之）。
- 任何失败都不得让 `PublishedTick.tick` 或 `generation` 倒退。
- **任何路径不得撤销已逐拍发布或已交付宿主的 tick/事件**；本文禁止使用“撤销一整天
  已发布交易”这类表述。日结失败只撤销“未最终提交的日结操作”。
- “本批尚未广播”不等于内部提交可撤。安装后与 #121 下一 Start 部分发送后的终止边界均不得
  通过旧 checkpoint、重建 driver 或显式 `abort_day_end` 绕过；销毁时正常停止线程也不得
  额外清算、推进自然日或发布业务事件。
- poison/terminal 后所有正常计算、查询、存档入口先 `require_healthy`，返回同一个首错。
  只允许错误诊断/关闭和显式读档或新局；诊断须标出市场来源版本、civil 日期与失败阶段，
  不能把留存的日结市场镜像显示为健康的完整自然日日终态。
- terminal 不授权清除已承认的市场成交/收据事实。尚未交付宿主的范围须随错误明示并保留
  原始事实供诊断；销毁协议的 Final 只能作诊断，不能另行发布业务版本或补造一次成功日结。
- 旧候选“内容保留”与 `save()`“仍可成功读取”不同：现有 `save()/save_candidate()` 均先健康检查；
  terminal 后不得绕过健康门。此前已捕获的不可变候选与有效持久档不会被失败替换。

---

## 3. 公司链往返

顺序（与 `RuntimeDriver::day_end` + `GameSession::end_civil_day` 对齐）：

```text
收盘竞价结算（World Drain）
  → 账户/持仓权威化（组端 Drain 回流 + T+1 解锁 + AdminBatch 权益）
  → 收盘事实归档（终 K 补真、drained trades/receipts、保留快照冻结）
  → 【同步汇合】flush_candidate（§1.2；日结路径必为 Full 来源）→ 单点安装
  → 自然日经营 end_civil_day（candidate clone → 校验 → 一次安装）
       ├─ CivilClock 推进（18:00 相位）
       ├─ CompanyOperations.advance_civil_day（当日到期经营，恰一次）
       ├─ close_accounting_periods（月/年末封账，如触发）
       └─ DisclosureDispatch.run_day_end（公告先于定期报告）
  → 存档候选生成（仅成功后；不可变；失败不得替换旧候选）
```

| 步骤 | 输入 | 输出 | 失败处理 |
|---|---|---|---|
| 收盘结算 | 当日全部在途/成交/book | Drain 事实、收盘价、终 K、退款实体 | 见 §2 #4–#8 |
| 账户权威化 + 汇合 | 组 `GroupAccount`、实体回流、累积 dirty | World 权威态的 session 镜像（一次安装） | 失败→candidate 丢弃、accumulator 保留、poison（§2 #9/#10） |
| 收盘归档 | 终 K、成交、竞价事实 | `RetainedIndex` 原子换新、`DayBoundary` 事件 | 准备失败→按 §2 #8/#9；已发布历史不可撤，不生成存档候选 |
| 自然日经营 | 冻结日历、公司到期日程、已同步账户权益 | 经营结果、披露公告 | `end_civil_day` 内部 candidate 失败→日历不推进；按 §2 #11 保留已装市场版本并终止，不撤市场日结 |
| 存档候选 | 已完成自然日 + 已安装镜像 | 不可变 `SaveSlot` | 失败→保留上一份有效存档；显式报错 |

**硬约束**：
- `end_civil_day` 之前必须完成账户权威化与**同步汇合**；否则公司链读到旧权益。
- `DayEnd` 策略下，日界拍的汇合必须是**同步完成**后才能进入 `end_civil_day` 与存档；
  **禁止**依赖当前实现“下一拍才全量回写”的延迟语义（`force_full_accounts`，`drive.rs:1268`
  置位、`drive.rs:866` 下一 step 清除）。实现批必须把日界账户回写移入 `flush_candidate`，
  `force_full_accounts` 仅可作为“来源选择提示”，不得作为跨拍延迟机制。
- 存档候选只能在 `end_civil_day` **成功之后**生成（ADR-0025）。
- `end_civil_day` 失败不得回到旧账户/T+1 镜像；成功安装的市场日结与失败的自然日日结是
  两个阶段。直接调用 driver 也必须在返回错误前设置 terminal，不能仅靠 server 来停止推进。
- 所有候选读取先健康检查；本合同不增加“失败后绕过健康门导出旧档”的入口。
- 公司链所需读取（账户、收盘事实、保留历史）一律走权威源，不读可推迟的展示副本。

### 存档 API 决策表（唯一规则）

现有公共入口在 `ProtocolSession`（`civil/session.rs:308-350`），`save()` 与
`save_candidate(key)` 均为 `&self`。它们只捕获已成功日结的候选，不编排可变 driver，
**不 flush、不推进自然日**；现有 `save_candidate` 成功捕获后移除 pending 引用的行为保持。

| 请求意图 / 入口 | 健康会话的结果 | flush / 新候选 | 宿主必须说明 |
|---|---|---|---|
| 读取最后成功候选：现有 `save()` | 有候选则返回其原字节；从无候选为 `InvalidSave(3015)` | 均不触发；最新市场 tick 已推进也不把本请求改成 3018 | 返回值的 `civil_clock.settled_through` 和 `snapshot.seq` 是真实保存边界；只能称“已完成某日的存档” |
| 捕获指定候选：现有 `save_candidate(SaveCandidateKey { settled_date, seq })` | seq 不存在/失效为 3016；seq 存在但日期不符为 3017；匹配则返回精确候选 | 均不触发；不因当前镜像滞后拒绝一个有效旧候选 | 自动日终写入必须使用通知携带的精确 key，不能换成 `save()` 取其他日 |
| 显式请求当前日：新增只读 `save_current_day(expected_settled_date)` | 仅最后成功候选的 `settled_through == expected_settled_date` 时返回；否则一律 `InvalidSave(3018)`，包括首个日结前 | 均不触发；即使手动 flush 成功，未完成该自然日日结仍为 3018 | 请求携带宿主明示的目标自然日；失败说明最新日终候选不匹配目标日，不能返回上一日冒充成功；历史读取用精确 key |
| 日内用户点击“保存” | 按 ADR-0025 只安排日终保存/选择输出目标；此刻不写盘，也不显示已保存当前进度 | 不 flush、不生成新候选；候选由后续成功自然日日结产生 | 明确显示“已安排日终保存”；若是显式导出已有档，须先说明实际日终日期/seq |
| 成功日终生成/捕获后写入文件或 DB | 使用同版不可变候选；写入失败显式报错并保留旧有效档 | 普通读取不参与此编排 | 未完成实际写入不得宣称已保存 |
| 任一入口遇 terminal | 首先返回保存的首错，不降级为 3015–3018，不返回旧候选 | 不触发任何计算/写入 | 旧候选内容仍保留不等于失败会话继续健康可读 |

`expected_settled_date` 是明确请求的自然日，不由最新市场 tick 或 session 镜像世代猜测；
实现批须校验日期合法。新增 3018 `SAVE_TARGET_DAY_NOT_READY` 表达没有与目标日匹配的最新日终候选，
不再命名为 `SAVE_NOT_FLUSHED`；须按 ADR-0041 登记数值错误码、上下文及宿主中文映射。
三档共用本表；不为存档读取添加 `Save` flush trigger，也不把一次镜像 flush 当作日终完成。

---

## 4. 三档时序与必须共同保留的检查

配置位置：`SessionSetup` 新增 `runtime_sync_policy`（`#[serde(default)]`）；不放入
`GameConfig`（A 股规则面）或 `WorldSpec`（撮合面）。缺省值按宿主入口区分，不做“存档补齐”。

```rust
pub enum RuntimeSyncPolicy {
    EveryTick,  // 交互宿主默认
    DayEnd,     // 非日界拍跳过可选镜像安装；日界拍一次同步 flush
    OnDemand,   // 显式 flush/运行账户消费点安装；日结同步 flush 三档共用
}
```

### 4.1 `EveryTick`

```
每 tick:
  World.run_ticks(1) → commit/publish（tick 级）
  → 事件组装与交付（seq 单调）
  → flush_candidate(trigger=EveryTick)（§1.2 唯一入口；来源由 account_source_reason 决定）
  → swap 前失败：丢弃 candidate + terminal；swap 后不得回滚已安装版本
日界后续：三档共用下述日结路径（末拍镜像不代替日结后镜像）
```

**v2 修订（S1）**：不再写“全量或增量安装”这种二选一描述；三档都调用
`flush_candidate`，`EveryTick` 只是每拍调用一次。

### 4.2 `DayEnd`

```
非日界 tick:
  World.run_ticks(1) → commit/publish
  → 事件组装与交付（必须）
  → 覆盖账户/belief/attention 最新值，置 plans_pending；事实仍完整移交，不安装镜像
  → 不做 market/candle/mirror/retail 安装，不做每拍 baseline/undo 快照
日界 tick:
  World.run_ticks(1) → commit/publish
  → 事件交付
  → 日结（World Drain、T+1/日 K/收盘事实）
  → 【同步 flush_candidate(trigger=DayEnd)】账户 Full + belief/attention 覆盖合并 + plans 完整读取
     + market/candle/mirror；校验通过后单点 swap
  → end_civil_day（公司链）
  → civil 成功：同步 World civil → 完整日终候选
     真实更正 prepare 两类业务拒绝且未提交/无首fatal：仅取消坏更正后 civil-only 重试
     其它 civil 失败：保留已装市场版本 + terminal；不撤安装、不发 civil 成功、不替换旧候选
```

**v2 修订（§3 硬约束）**：日界 flush 必须在 `end_civil_day` 之前同步完成；不得推迟到下一拍。
这是三档共同的领域屏障，`DayEnd` 的禁令只针对日内玩家 ingress，不能屏蔽公司链或日终候选。

### 4.3 `OnDemand`（v2 修订，闭合 L3）

```
普通 tick: 同 DayEnd 的非日界路径（只累积、不安装）
日结: 无条件执行 §4.2 的同步 flush → 公司链 → 日终候选，不能等诊断查询触发
日内可选 flush（以下唯一触发源）:
  1) 宿主显式调用 flush_now()
  2) 运行账户消费点（见 §4.6）在读取前同步 flush
未 flush 时:
  → 诊断/展示读面返回“上次安装版本 + 真实来源版本（含日结阶段）”，
    不得用最新 tick 包装旧数据
只读存档请求: 只执行 §3 API 表，不触发 flush 或自然日日结
```

- **不存在**“任意外部读都触发汇合”的规则；只读历史候选与诊断读不触发。
- flush 失败时，计算消费点操作必须**拒绝**（显式错误），不得按旧镜像继续。

### 4.4 三档必须共同保留的检查

无论哪一档，以下**不得跳过**：
1. `World` 每拍 Start/收敛/Seal/SealAck 与提交/发布（`PublishedTick`）。
2. 事件 seq 单调分配与每拍交付顺序。
3. 日界领域迁移：T+1 解锁、活动 K 提交、收盘竞价清算、`day` 推进、保留日冻结。
4. 日结 `AbortDayEnd` 边界与“不倒退已发布 tick”。
5. 公司链 `end_civil_day` 与存档候选一致性。
6. 任何安装的“禁止半安装”与失败 poison。
7. 账户读面在**运行账户消费点**（§4.6）必须是最新权威版本：
   - `EveryTick`：天然满足；
   - `DayEnd`：仅允许无日内玩家 ingress 的批量仿真；出现 ingress 显式拒绝（§4.6）；
   - `OnDemand`：运行账户消费点读取前同步 flush；诊断读面可滞后。
8. 三档均在日结领域屏障同步 flush，再执行公司链与成功日终候选生成；只读候选 API 不 flush。

### 4.5 默认值策略（未翻转）

- 交互宿主（Web/远程/桌面）：默认 `EveryTick`。
- 无头批量仿真（server 仿真、基准、回放）：显式选择 `DayEnd`。
- 在取得完整跨日配对数据、并通过**单独的默认值决策门**之前，不翻转任何全局默认；
  `OnDemand` 只作显式选项。

### 4.6 OnDemand 读面分类与 DayEnd ingress 行为（v2 新增，闭合 P2-1 / L3）

| 类别 / 入口 | EveryTick | DayEnd | OnDemand |
|---|---|---|---|
| 日内运行账户消费：玩家下单/撤单及其现金、持仓、可卖校验 | 用当前安装版本；不重复 flush | 请求进入 pending/receipt/resource 校验前显式拒绝，不 flush | 先同步 flush，再受理/校验；失败拒绝并 terminal |
| 必需日结消费：公司链 `end_civil_day`、权益/收盘事实与自然日日终候选生成 | 先共同日结 flush | **允许且必须**在共同日结 flush 后执行 | **允许且必须**在共同日结 flush 后执行 |
| 显式同步 API `flush_now()` | 允许 | 允许显式同步镜像，不能因此开启玩家 ingress | 允许 |
| 只读日终事实：`save()` / `save_candidate(key)` / `save_current_day(date)` | §3 决策表 | §3 决策表；旧候选读取不触发日内同步 | §3 决策表；不能因调用 save 隐式同步 |

休市日也必须执行现有自然日日结，使用已经权威同步的账户和日程；没有市场 tick 不得
虚构交易、再次 T+1 解锁或跳过公司链。日终候选只在完整自然日成功后生成。

**诊断/展示读面（不 flush，返回真实 `(tick, generation)`）**：
1. `snapshot`/查询类只读接口；
2. 历史与 K 线查询（按日期/游标，不依赖镜像新鲜度）；
3. 速度/指标类诊断（`speed_metrics` 等）。

会话进度仍按提交 tick/公开 seq 前进；snapshot 若同时含进度与滞后镜像，必须分别给出
进度游标与镜像来源（含日结阶段），不能把整份返回值标成同一个最新版本。历史查询按其
独立日期/游标表达完整性，不以镜像滞后暗示历史缺失；terminal 仍先走健康门。

**DayEnd 下的 ingress 行为（唯一选择）**：
- **显式拒绝**。玩家下单/撤单请求返回 `SessionError::SyncPolicyViolation(ValidationFault)`
  （新增枚举，数值码在实现批登记；ADR-0041），附带 `policy`、`tick`、`generation` 数值上下文；
- 拒绝时账户、镜像、来源版本、pending/receipt 游标均不得变化；
- **不采用**“强制 flush 后受理”作为 DayEnd 的行为（该行为只在 OnDemand 下存在）。

---

## 5. 版本合同（v2 补充精确条件）

1. **`PublishedTick.tick` 单调不减**：每拍提交前进；任何失败路径不得回退。
2. **`generation` 严格递增**：World 每次发布前将 `publish_generation` +1（`world.rs:832,2098`），
   `ViewPublication::publish` 只安装传入版本（`publication.rs:230`），不负责生成世代。
   同 tick 重发只允许两种情况：(a) 日结保留链冻结后重发；(b) civil 日期推进后的重发。
   断言：`(tick_new, generation_new)` 字典序严格大于旧值；若 `tick_new == tick_old`，
   相邻两次 publish 必须 `generation_new == generation_old + 1`，不得复用；读取者跳过中间
   发布时只要求严格增大。#121 的日结 `public_views` 更新本身不新增 PublishedTick。
3. **镜像世代不可倒退**：安装记录 `(tick, generation)` 及日结阶段来源；禁止更旧世代或
   日结前阶段覆盖已安装日结阶段。按 §2 终止也不能回退；旧候选有自己独立的日期/seq。
4. **存档候选世代不可倒退**：只有在自然日成功后生成新候选；失败日结/失败写入保留上一候选，
   异步写入的内容必须来自该不可变候选（ADR-0025 第 4、6 条）。
5. **事件游标与镜像游标分离**：`seq` 只增不减；镜像滞后不代表事件未交付。
6. **不得用最新 tick/seq 包装旧账户/旧行情**：任何读面必须同时暴露其真实
   来源版本（普通拍 `(tick, generation)`，日结加阶段），宿主据此判断新鲜度。
7. **civil date 权威**：`world.civil_date()` 只是 session 的投影；任何一致性校验以
   `session.civil_date()` 为准（§1.0）。
8. **一次权威**：World 负责市场账户/计划/attention 计算，session civil 负责自然日；
   driver 只维护镜像候选与传递游标。session 运行 tick 仍与 World 锁步，可选镜像另记来源；
   实现批须同步 ADR-0043 决策 7/8 与正式架构的滞后读面说明，不暗改同拍资金可用语义。

---

## 6. 验收用例矩阵（TDD 先行，红→绿）

> 全部用例先写失败测试（红），再实现（绿）；不允许弱化断言。
> #1–#16、#18 为普通用例（单 case 与整命令硬上限 10s）；#17 为长验收（见 §7）。

| # | 用例 | 断言（精确） | 适用档位 |
|---|---|---|---|
| 1 | 早拍变更不丢 | 同一账户在窗口内多次成交后，日终 flush 的现金/持仓/T+1 冻结量与 `snapshot_accounts()` **逐分逐股相等**；任一字段不等即失败 | DayEnd / OnDemand |
| 2 | 收盘成交完整归档 | 日终 `drained_trades` 数量与逐笔 `(stock, price, qty)` 等于 World 当日收盘成交集合；fixture 成交 200 股时断言总量恰为 200，且无重复交付 | 三档 |
| 3 | 日界后首拍正确 | 保持 #121 原断言：`public_views` = `(upper=1210, lower=990)`（close=1100）；Highest 买单 `purse == 121501` 分；desk 受理不报预留失同步 | 三档 |
| 4 | 失败无半安装 | 在私有 candidate 的 market/candle 构建或校验点注入失败：各域、来源阶段与覆盖表等于失败前；swap 次数为 0；World 已发布 tick/generation 不倒退；不能借现有“先写行情再撤销”路径冒充通过 | 三档 |
| 5 | 旧存档候选不变 | 注入前捕获候选与内部候选引用；注入后候选内容/日期/seq 字节级相同，未产生新 key；公共 `save()/save_candidate()` 返回同一 terminal 首错，不能把“保留”断言写成失败后 save 仍成功 | 三档 |
| 6 | 日结 Drain/组失败边界 | 保持 #121 三种注入：组游标严格 `[(0,240),(1,240)]`；未收到 `DayEnd` 的组不回滚 | 三档 |
| 7 | 统一汇合等价性（**同一次运行内**） | 从同一固定 World T 与同一安装前镜像分别构建正常来源/强制 `Full` candidate，`accounts/personal/plans/attention/markets/candle_book/desk_book_mirror/resting_snapshot` 逐字段相等；分别在 swap 前注入同一失败，未改镜像与覆盖表逐字段相等；不能用已清表的第二次 no-op 或另跑自由并发世界替代 | 三档 |
| 8 | 跨档对照（**确定性 fixture**） | fixture：单组、单 desk、固定脚本、现有锁步通道；`EveryTick` 与 `DayEnd` 在相同 tick 序列的日终快照（账户、持仓、K、行情、事件集）逐字段相等。**声明：该 fixture 不代表自由并发契约，不得据此声称任意并发下等价** | EveryTick vs DayEnd |
| 9 | OnDemand 读新鲜度 | 未 flush 时 `snapshot()` 返回旧 `(tick,generation)` 且现金为旧值；计算消费点调用时同步汇合，返回新值；现金差精确等于本窗口成交额 + 按 `buy_purse_limit` 计算的边际费用（断言与函数返回值分分相等） | OnDemand |
| 10 | DayEnd ingress 拒绝 / OnDemand 受理 | DayEnd 下分别下单/撤单均返回 `SessionError::SyncPolicyViolation(_)`，账户/镜像/来源阶段/pending/receipt 游标不变，flush 次数为 0；显式 `flush_now()` 可成功但不改变拒绝策略。OnDemand 同一请求先 flush 后受理，`purse == buy_purse_limit(config, price, qty, ZERO).cents()`，余额不足为精确 `InsufficientFunds` | DayEnd / OnDemand |
| 11 | 公司链与存档准入（写死 fixture） | 保留既有权益 fixture：1000 股×10 分使账户现金恰增 10,000 分；披露事件为 `[公告, 定期报告]`，连续 seq 各一次；DayEnd 与 OnDemand 无任何玩家/诊断请求也必须依次执行日结 flush→公司链→日终候选，`save_current_day(该已结日期)` 成功，精确 key 与公告日匹配；休市日不虚构成交/T+1 日结，照常完成自然日与候选 | 三档 |
| 12 | terminal 后行为 | `require_healthy()` 返回注入的 `StepFatal::InvariantViolation { location, description }`；连续 step/flush/day_end/普通查询/save/save_candidate/save_current_day 均返回同一首错，未发 Start/Abort，无状态或 seq 变化；不得通过重建 driver/旧 checkpoint 继续。阶段保留断言见 #18 | 三档 |
| 13 | 有界覆盖与计划现态 | A 个账户连续 N 与 2N 拍反复变化：账户/belief/attention 各表每账户≤1条，合计≤3A，加一个 plans 标记；重复同序同值 no-op，同序异值/旧序/未知账号/缺项显式失败。计划建立→修改→终结（退出 active 索引）后，完整现态与含 terminal 的权威簿相等；真实空簿清除组权威旧镜像但保留明确 session 自有计划。成功 swap 后清表/复位标记；覆盖表不出现在读面 | DayEnd / OnDemand |
| 14 | 事实完整处理与交付 | 窗口内同一账户多次成交、同一计划多次变更/终结：每个公开交易/收据事实按身份恰一次移交，完整 `seq` 与已承诺历史不丢失/重复；内部 PlanEvent 按原 tick/轮次完整执行，terminal 状态与权威簿相等，不用公共事件 seq 冒充内部计划日志。交付失败显式终止，不能清表代替处理；事实保留不受3A限制，日内不新增持久写入 | DayEnd / OnDemand |
| 15 | generation 单调 | 同 tick 重发时 `generation_new == generation_old + 1`；`(tick,generation)` 字典序严格递增；尝试安装旧世代 → 显式拒绝 | 三档 |
| 16 | 存档请求意图与错误码 | 健康会话：无候选的 `save()`=3015；未知 key=3016；已知 seq 错日期=3017。下一日 tick 已提交但未日结：`save()`仍返回原候选/原日期/seq，精确旧key可捕获；`save_current_day(未结日期)`=3018（首日无候选也3018），即使 `flush_now()`成功仍3018；目标日成功后仅返回该日候选。所有只读请求 flush次、Start次与市场状态不变；宿主日内点击只安排日终，不写盘/不报当前进度已保存 | 三档 |
| 17 | 配对测量（**非通过/失败门**） | 只产出 §7 指标数据；**不设通过/失败阈值**；不得据此宣称收益承诺或翻转默认 | DayEnd |
| 18 | 安装后 civil 失败的不可撤边界 | 直接 driver 与宿主两层分别注入：日结 Full flush 已成功、civil candidate 失败。账户/T+1/markets/K/mirror/resting/日结来源阶段均与刚安装版本逐字段相等，覆盖表保持成功清空；World日结/已发布tick不回退、civil日期不前进，旧候选字节/key不变；已提交市场事实保留且未交付范围显式报错，未提交civil/收盘输出不重复交付。后续任何推进/撤销/重建driver续跑都失败，旧checkpoint不得复活；另复用#121下一Start部分发送失败测试，确保已转正参与者不被撤销、dead_reason阻断后续入口 | 三档 |

**资源分类（闭合 P2-3）**：
- #1–#16、#18：普通用例。每个 case 与整命令进程树硬上限 **10s**；按测试二进制/套件多核并发；
  fixture 必须最小化（≤ 2 组、≤ 2 股、≤ 3 个交易日）。
- #17：**长验收**。共享 `300000ms` deadline，其中执行/校验/发布 ≤ `299000ms`，
  最后 `1000ms` 仅用于终止与清理；必须由进程外 supervisor 执行；测量期独占整机
  （其他 agent/构建暂停），记录实际并发参数和 CPU/IO 利用率。
- 禁止把长验收伪装成普通单测，也禁止把普通用例串行单核跑。

**验收纪律**：
- §0 独立身份硬前置须先交付并获可信独立复核；下表/性能测量不能替代该实施入口门禁。
- 每例先写失败测试（红），再实现（绿）；不允许弱化断言。
- 每批实现完成后由未参与实施的 subagent 独立复核完整 diff（AGENTS.md）。
- 性能数据必须绑定 binary SHA、测量窗 load、配对中位数。

---

## 7. 配对测量计划（决定默认值前必须完成；本轮不设门）

**v2 修订（S4/L4）**：本节**不声明**通过阈值，也不作为本轮通过/失败门。它只规定必须采集的数据，
供后续**单独的默认值决策门**使用；在该决策门获得用户批准前，任何默认值不得翻转。

1. **配对口径**：同一 binary（绑定 SHA256）、同一 seed/配置，ABBA 交替
   `EveryTick` vs `DayEnd`，**至少 5 轮**，剔除热身轮；测量期独占整机并记录 load average。
2. **覆盖范围**：必须包含“完整交易日（含日界）+ 自然日日结 + 下一交易日首拍”，
   不得只用不触发日结的短窗。
3. **计入项**：flush 全量账户读取、belief/attention 覆盖合并、plans 完整读取、行情/K/mirror 安装、
   历史游标消费、事件交付；同时记录内存峰值。
4. **指标**：
   - 日内稳态 t/s（去掉热身段）；
   - 日终 flush 峰值延迟（P50/P95/max）；
   - 整日 wall（含 flush + civil day）；
   - 相对 `EveryTick` 的吞吐提升；
   - 内存增量与**随 tick 数的增长斜率**。
5. **覆盖表判定与内存测量分开**：固定名册 A 时，三张覆盖表合计≤3A，另一个 plans 标记；
   N/2N 拍均不得超界。分别测覆盖表字节、权威 PlanBook（含 terminal）、既有交易/收据事实、
   保留历史和总 RSS；事实增长必须归因并核对保留范围，不能以 RSS 增长直接判错或删除历史。
   本轮不设总 RSS、吞吐或日结延迟的性能通过阈值。
6. **决策规则**：只有“语义用例 #1–#16/#18 全绿 + 配对数据完整 + 默认值决策门获批”
   才考虑翻转默认；实测 25.1% 是**可移上限**，不是承诺收益。
7. **B 探针限制**：`ticks_per_day=15300` 不触发日结，不能单独作为 `DayEnd` 收益证据；
   必须补跨日 fixture。

---

## 8. 明确简化 / 不支持

1. **`DayEnd` 不支持交互宿主日内下单**：玩家 ingress 依赖最新 session 账户；
   该场景必须 `EveryTick` 或 `OnDemand`；`DayEnd` 下 ingress 请求显式拒绝
   （`SessionError::SyncPolicyViolation`），不实现“后台悄悄 flush”。
2. **`OnDemand` 的“外部读”边界**：只有 §4.6 的显式 flush API、运行账户消费点与三档共同日结屏障触发汇合；
   **不对诊断/展示读自动触发**（v2 统一规则，取代 v1 的并存表述）。
3. **事件不批量延迟交付**：本设计只推迟镜像安装，事件仍每拍交付；跨日批量事件流不支持。
4. **跨档热切换**：不支持运行中切换 `RuntimeSyncPolicy`；策略随 `SessionSetup` 固化。
5. **存档格式兼容**：新增字段的 `#[serde(default)]` 只表示运行配置缺省，不给存档补字段；
   公共存档入口仍严格拒绝旧字段、不完整结构及额外版本字段，不建立迁移或兼容运行路径。
6. **wasm 串行管线**：`wasm` 无 OS 线程、无 `World` 提交/发布分片，本合同只适用于
   `std-thread` runtime 形态；wasm 串行路径保持 `EveryTick` 语义（登记为不支持日内解耦）。
7. **公司链批量汇合**：不支持“多个自然日一次性补公司链”；每个自然日必须逐日成功。
8. **`flush`/安装后 civil 失败的恢复**：原会话不支持重试、自动回放、重建 driver 或
   checkpoint 复活；只能关闭后由宿主显式读档/新局，并保持用户可见的恢复边界。

---

## 9. 与 Sol 裁决的对应关系

| Sol 裁决项 | 本合同的落实位置 |
|---|---|
| 1 边界判据 | §0 硬前置、§4.4 第 7 条、§4.6 |
| 多组计划身份阻断 | §0 独立身份硬前置；身份设计与交付验收独立于本合同条件通过，未交付禁止开启 #122 实现 |
| 2 三层原子边界 | §1.2-A 安装边界、§2 失败矩阵及 #18 用例；安装后 civil 失败不沿用旧撤销函数 |
| 2 完整权威读取（不能只装最后一个 PublishedTick） | §1.0–1.2 |
| 2 公司链覆盖 | §3 |
| 3 save 只由成功日终生成 | §3 API 决策表（3015/3016/3017/3018）、§5 第 4 条 |
| 4 三档共用同一实现、共同检查 | §1.2-A、§4.1–4.4 |
| 5 默认值需配对数据 | §4.5、§7 |
| 6 保留每拍 publish、不自动降级交互宿主 | §0、§4.4、§5、§8 第 1 条 |
| 7 先修 #121 | 最终修订先集成并独立复核；覆盖下一 Start 部分发送不可撤与 dead_reason，不能沿用旧轮次“已完成”结论 |
