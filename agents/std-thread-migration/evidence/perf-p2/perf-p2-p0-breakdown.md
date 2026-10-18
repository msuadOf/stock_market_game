# P2 P0 级分解：B 口径（driver_semantics_probe）每 tick 成本画像

日期 2026-10-17。基线=P1 后 HEAD（816cddb3，tag perf/bare-1358x）。
采样：`perf record -F 499 --call-graph dwarf,16384` 600 tick / groups=1 / 20000 retail NPC
（`.tmp/perf-records/p2-baseline-b.perfdata`，60126 样本）。
注：dwarf unwind 在该二进制上断栈（用户态调用树不可用）——本报告基于
perf report 周期加权叶帧 + 代码结构对照，占比为叶帧口径。

## 总量形态（与 A 口径完全不同）

| 面 | 占比 | 说明 |
|---|---:|---|
| **驱动器线程（主线程）** | **72.8%** | 单线程≈wall 关键路径（B 探针主循环） |
| 组线程（rt-group-0，仅 1 组） | 27.0% | 决策+组侧上行 clone |
| 内核态 | 11.9%（DSO 口径） | 同步税不主导（P1-A 已消广播税） |

## 驱动器线程符号构成（叶帧，周期加权）

| 成分 | 占比 | 归属 |
|---|---:|---|
| BeliefEntry::clone | 8.87% | 个人态安装链 |
| BTreeMap clone::clone_subtree（4 实例） | ≈21.8% | 同上（watchlist/belief/positions 等深树） |
| __memmove_avx | 6.01% | 深拷贝 memcpy |
| BTreeMap IntoIter::dying_next（4 实例） | ≈9.1% | 深拷贝释放 |
| jemalloc 族（malloc/free/tcache/eset） | ≈12% | 深拷贝分配 |
| **深拷贝+分配小计** | **≈58%** | **全名册个人态投影安装链** |
| merged_with_group_authority | 1.47% | retail 经验行合并 |
| AccountPagedMap::remove（2 实例） | ≈1.8% | set_runtime_personal_state 的 remove+insert |
| AccountBook::get | 1.04% | 账户回写读面 |

组线程 27% 中另含 BeliefEntry::clone ≈3%+build_market_view 等（组侧 EchoBelief
整组 clone 上行+链位观察构建）。

## 代码对照（深拷贝链全景，每 tick × 20000 NPC）

1. 组侧 `EchoBelief`（group.rs:4309）：`state.belief_states.clone()` 整组深拷贝上行。
2. 驱动器 `collect_group_belief` → 每 NPC `set_runtime_personal_state`
   （failure.rs:77-114）：attention.clone + belief_participants remove/insert
   （watchlist/price_memory/information/**belief** 四件深 clone）+ history_reads.clone
   + retail_experience remove+merge+insert。
3. 账户链 `snapshot_accounts`（EchoAccounts 整组上行）→ drive.rs:500-575 每 NPC
   positions.clone+merged 回写（第二嫌疑，符号面 AccountBook::get/clone_subtree 部分）。

## 结论与动刀顺序（按占比）

1. **P2-1a 个人态 dirty 化**（EchoBelief 只上行本 tick 演进 NPC+安装侧只碰 dirty）
   ——≈58% 的大头，Sol 估 B +30-60% 的主要来源。
2. **P2-1b 账户 dirty 化**（EchoAccounts/回写只碰本 tick 落账变化账户）——第二。
3. **P2-2 链位观察共享**（build_chain_observation 的 market 构建并共享——P1-B 同式
   延伸到链位）——组线程侧。
4. **P2-3 B 探针 fee sink 强对账**（欠账）。

阶梯目标（Sol 区间）：B 从 5.02 → 20-50（P2-1a/1b）→ 60-150（P2-2 叠加+后续）。
