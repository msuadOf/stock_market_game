# 语义矩阵缺口测试草案说明（R23/R24/R30/R33）

- 产出：`.tmp/gap-test-drafts.rs`（8 个测试函数草案，按目标落地文件分 4 节）+ 本说明。
- 依据：`agents/std-thread-migration/plans/haiku-traceability-refresh.md`（2026-10-18，
  刷新结论「R30、R33、R23、R24 仍为部分覆盖」）与 `plans/semantic-traceability.md`（规则定义）。
- 约束遵守：只读分析，未修改任何源码/测试，未运行 cargo。所有 API 签名均对照现有
  测试/源码逐个核对（见各节「落地核对」标注）；草案以「可编译为参考」为目标，
  个别 const fn 与 fixture 有效性点需落地时现场确认。

## 0. 编号澄清（任务书与矩阵文档的不一致，如实登记）

任务书 item 3 把「计划分类观察节奏」标作 R23、「定时自动」标作 R24。按两份矩阵文档：
- 计划分类观察节奏 = **R22b**（refresh 已判**已覆盖**：factory.rs
  `r22b_classification_cadence_matches_registered_policy`:237、
  `r22b_belief_horizon_days_by_classification`:289），无需新草案；
- 定时自动 = **R30**。

四面缺口集合以 refresh 结论为准：**R23 受理先后 / R24 计划根逐个完成与 terminal
生命周期 / R30 定时自动（引擎面）/ R33 尚未模拟负向范围**。本草案按此执行；item 3
要求的「注入通道说明」在第 5 节对全部相关面给出。

## 1. R23 受理先后（缺口：来源标签非优先级、同账户按接收事实）

### 1.1 `r23_same_account_cash_adjudicates_by_receipt_order`（确定性 oracle）

- **验证意图**：同账户现金裁决跟随请求**接收先后**，不按订单 ID、来源标签或任何
  固定优先级（trading-rules.md 受理先后段：「三阶段的同账户资源争用已从候选数组
  位置改为接收事实……这个比较只用于同账户现金或逐股可卖股份」）。
- **fixture**：`setup_fixture` 纯玩家化（retail=0/inst=0、auction_ticks=0、
  starting_cash=200_502 分），两批注入（正向 [A,B] / 反向 [B,A]）各跑一遍。
- **oracle 计算过程**（proposed_defaults：佣金 max(0.00025×额,500)、过户 0.00001×额）：
  - A=买 100@1000：purse = 100_000 + max(25,500)=500 + 1 = **100_501**；
  - B=买 200@1000：purse = 200_000 + 500 + 2 = **200_502**；
  - 现金恰=B purse ⇒ 正向批 [A,B]：A 受理、B 拒 InsufficientCash，
    期末现金 200_502−100_501=**100_001**；反向批 [B,A]：B 受理、A 拒、期末 **0**。
  - 判别逻辑：若实现按 ID/来源固定排序，两批结果相同 ⇒ 断言红。
- **断言**：恰一条 OrderAccepted（remaining_qty 随批序翻转 100↔200）、恰一条
  InsufficientCash 回执（不静默）、组快照现金精确等于手算值。精确值无容差。
- **落地注意**：目标文件 `src/runtime/drive_tests.rs`。同批 FIFO 是既有事实
  （anchor 测试注释「批内 FIFO：卖先挂、市价买随后吃单——确定性」）。

### 1.2 `r23_cross_source_acceptance_follows_entry_not_label`（结构性断言）

- **验证意图**：「不同账户争同一股票的受理先后由该股入口实际发生的先后决定，
  来源标签与订单 ID 不充当优先级」。NPC 侧（belief 链 + `force_due_for_test_at`
  定时决策）与玩家侧（`inject_player_intents`）同拍申报同股。
- **断言（赢家无关——tick 内任意是既定策略，见 project_same_price_winner_policy）**：
  (a) belief 位本拍受理 ≥1 单（前置，防断言空转）；(a') 玩家单「受理或成交」必居
  其一且无任何回执（铁律二：不静默丢弃）；(b) 同股 NPC 受理 seq < 玩家受理 seq
  ——组合向量序=入口序，按来源/ID 重排会翻转 seq 序而显红。
- **落地注意**：目标文件同上。(b) 仅在玩家单以挂簿受理时可比（吃单成交无
  OrderAccepted），代码已条件化。**风险**：belief 位在暖机 3 拍后 force_due 是否
  必产单依赖种子——前置断言故意大声失败（换种子即可，不静默绿）。

## 2. R24 计划根逐个完成交付 / terminal 生命周期

已有锚：`sol4th_full_fill_completes_plan_to_terminal`（group.rs:5121，单根
Completed）、solmid/sol4 系（执行腿成交推进/日终 Drain/Abort）。refresh 缺口=
「计划根逐个完成/terminal 生命周期未完整锚定」——本节补两面，均落在 group.rs
的 `sol_recheck3_tests` 模块（`use super::*` 环境，chain_harness_target 邻近；
新 helper `harness_roots`：1 账户×2 股双根，结构逐字对照 :5030）。

### 2.1 `r24_plan_roots_complete_individually_without_waiting_for_other_roots`

- **验证意图**：「计划根现逐个完成并交付，无须等同批其他账户的根都完成」——
  根 A 全额成交 ⇒ 独立 Completed；根 B 不被牵连（仍 Active、腿/在途票原样）。
- **oracle**：A fill 100/100 ⇒ filled_qty=**100**、腿清空、票释放；
  B 无事实 ⇒ filled_qty=**0**、active_child_remaining_qty=Some(**200**)、
  在途身份 order_id_of(&(1,5)) 原样、child_in_flight 键保留。
- **落地注意**：`batch_on` 是既有 `batch()` 的带股参数化（原函数硬编码 stock:0）；
  FillFact 形状对照 sol4th。落地建议：把 chain_harness_target 参数化，避免两份
  RootProjection 字面量。

### 2.2 `r24_terminal_record_persists_and_frees_slot_for_new_root`

- **验证意图**：terminal 生命周期下沿——Completed 记录**保留在簿**（按 id 可查、
  状态不回退），(账户,股) 槽位即时释放，同键可开新根且 PlanId 单调不复用
  （「逐个完成并交付」的下游效应：完成的根不阻塞新计划）。
- **oracle**：plan(pid_a).status==Completed（完成前后两次断言）；
  active_plan(...)==None（释槽）→ create 同键新根 Ok 且 pid_new.0 > pid_a.0；
  active_face 指向新根。PlanId(pub u64) 已核对可访问 .0。
- **落地注意**：PlanBook::create/plan/active_plan 均 pub（restore_guards.rs 有
  save.plans.create 同款直构先例）。

**残余缺口如实登记**：terminal 记录经交付点投影到 session `plans_debug` 的
端到端面（solmid_plan_projection 的 fixture 无 terminal 记录）仍未锚——drive 级
belief 链无法廉价产生 terminal（目标量级巨大、日终 Drain 不终止计划、horizon
到期需多日跨日 fixture）。可选补法：多日驱动等 horizon 到期，或等视图内生战役
Q1 发布协议落地后由读面等价测试承载；本轮不虚报已覆盖。

## 3. R30 定时自动（引擎面）

规则原文（trading-rules.md 2026-10-05）：「定时自动是会话内 Web 功能，不是交易所
原生委托类型，也不写入日终存档。」引擎侧三面：词表（已锚）、存档不含该域
（refresh 判缺）、Web 行为（到期执行/暂停无进度——**engine 范围外，by design**）。

### 3.1 词表编译期门（任务 item 2 写法草案）

三层结构（落地 `tests/scope_boundary.rs`）：
1. `const *_TABLE: [&str; N]`——注册处，新增变体必须改表；
2. `const _: () = assert!(TABLE.len() == N, "…")`——类型级哨兵钉表长
   （const 断言零运行时成本，链接期丢弃）；
3. `const fn *_name_const(&Enum)`——穷举 match，每变体恰一臂，新增变体 ⇒
   非穷举 ⇒ 编译失败。

运行期测试 `r30_r33_const_tables_match_runtime_vocabularies` 把 const 表与
serde 标签集对拍（序列化契约面双保险）。
**落地核对**：`&Intent`（含非 Copy 字段）在 const fn 中用 `..` 模式匹配应合法；
若编译器拒绝，退化为普通 fn——穷举编译门不损失。

### 3.2 `r30_day_end_save_carries_no_scheduled_automation_domain`

- **验证意图**：「不写日终存档」具名锚——refresh 明言该面未见单独断言。
- **fixture**：`runtime_events_with_driver(245, 703)` 跨一个日结（240 tick/日），
  `session.save()` 后 `serde_json::to_value` 全键递归扫描。
- **oracle**：无任何键（小写子串）含 `schedul` / `timer` / `automation`。
  token 集已对照现存 SaveSlot 键面无碰撞。deny_unknown_fields 是契约门（新字段
  必须改 SaveSlot），本测试是语义命名锚——新增定时自动域必须先修 R30 登记。
- **落地注意**：scope_boundary.rs 头部需补 `mod common;`（tests/common 可用，
  anchor 文件同款）与 `use engine::Event;`。

## 4. R33 尚未模拟负向范围（缺口：退整、新股无涨跌幅登记子面）

### 4.1 `r33_price_band_enforced_from_first_tick_of_existence`

- **验证意图**：「新股上市初期无涨跌幅限制不在范围」行为锚——股票存在的第一个
  交易 tick（世界构造=上市时点等价）即受带，无按上市日数豁免的路径。
- **oracle**（cage 关、@1000 主板 ±10% 带 [900,1100]）：首拍 1101 ⇒
  skipped_orders=**1**；tick3 成交 ⇒ 买方 1_000_000−**100_501**
  （100_000+佣金 500+过户 1）、卖方 1_000_000+**99_449**
  （100_000−500−50−1）。与 desk 既有测试同口径（matching/multi_round 手算一致）。
- **落地注意**：目标文件 `src/runtime/desk_semantics_tests.rs`（buy/sell/place_at/
  basic_spec/config_with_cage 均为该文件既有 helper）。

### 4.2 `r33_phase_vocabulary_closed_across_full_trading_days`

- **验证意图**：「退整/停复牌/盘中临停不在范围」运行面锚——跨完整交易日的事件面
  只出现四个合法相位（编译期门防新增变体，本锚防运行期出现表外相位）。
- **oracle**：245 tick 事件中 AuctionTick/AuctionCompleted 的 phase 名集 ==
  四相位 const 表（集合相等，精确）。

### 4.3 存档面不做键扫描（决策记录）

R33 的两融/大宗在存档的缺席由 Intent 词表门 + SaveSlot deny_unknown_fields
蕴含；曾考虑扫描 `margin` 等 token，但 `InstParams.margin`（信念链参数）为现存
合法字段，会假红——故不做，避免脆弱关键词断言（诚实登记，不硬凑覆盖数）。

## 5. 注入通道说明（任务 item 3）

「既有三通道」取视图战役纲要 Q4 的注入路径清单
（`plans/view-campaign-design-brief.md` §Q4：`InstallPersonal`、`AdminBatch`、
测试 fixture 通道，代码面 drive.rs:809 一带）。各面用法：

| 面 | NPC/引擎侧通道 | 玩家/宿主侧通道 | 说明 |
|---|---|---|---|
| R23 | belief 链 + `force_due_for_test_at`（测试 fixture 通道的 drive 形态）；World 侧=`WorldSpec.script_for` | `inject_player_intents`（InstallPersonal 载荷的生产等价面，route_external_intents→broadcast_external_plan）；`enqueue_player_intent`（session ingress 收据序，ADR-0032 家族） | 1.1 用玩家批内 FIFO（接收事实的确定性投影）；1.2 双通道同拍 |
| R24 | group.rs 组内 `plan_book` 直构（fixture 通道；drive 侧为 belief 链有机产生） | —（计划根属 NPC 信念链，玩家无计划） | 2.1/2.2 全走组内直构，不依赖时序 |
| R30 | 引擎**无通道**（by design：非交易所委托类型） | 到期执行未来属 Web 会话面：游戏秒进度到期后经 session ingress（enqueue_player_intent 家族）注入；存档面经 `save()` 契约（3.2 已锚「无域」） | Web 行为测试待 packages/web |
| R33 | World `script_for`（fixture 通道） | — | 4.1 desk 世界脚本 |
| R22b（若任务 item 3 本意指「计划分类观察节奏」） | factory.rs 纯参数直构（无注入通道——`build_institution(ordinal)` 直接构造策略对象） | — | refresh 已判覆盖，无新草案 |

## 6. 草案与 oracle 清单

| 草案 | 落地文件 | 手算数值锚 |
|---|---|---|
| r23_same_account_cash_adjudicates_by_receipt_order | src/runtime/drive_tests.rs | 4（100_501/200_502/100_001/0） |
| r23_cross_source_acceptance_follows_entry_not_label | 同上 | 0（结构性断言，seq 序） |
| r24_plan_roots_complete_individually_without_waiting_for_other_roots | src/runtime/group.rs | 2（filled 100/0、余量 200） |
| r24_terminal_record_persists_and_frees_slot_for_new_root | 同上 | 0（身份/状态断言） |
| r30_r33_const_tables_match_runtime_vocabularies | tests/scope_boundary.rs | 3（表长 3/4/3） |
| r30_day_end_save_carries_no_scheduled_automation_domain | 同上 | 0（键扫描） |
| r33_phase_vocabulary_closed_across_full_trading_days | 同上 | 0（集合相等） |
| r33_price_band_enforced_from_first_tick_of_existence | src/runtime/desk_semantics_tests.rs | 3（skip 1、100_501、99_449） |

合计 8 个测试函数、12 处手算数值 oracle（另有集合/seq 序等精确非数值断言）。

## 7. 落地门禁提醒

- 落地时走 TDD：先确认各断言在当前实现下**绿**（这些是补锚不是修 bug 的红测）；
  若红，先按「负结果先归因」核对是草案假设错还是实现缺口。
- 每批落地后按 CLAUDE.md 安排未实施 subagent 复核（大 A 语义+最小范围）。
- 本工作只读：未改源码/测试、未跑 cargo、未动引擎 worktree。
