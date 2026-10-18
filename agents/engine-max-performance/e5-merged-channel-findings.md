# 捕获通道合并实验（已撤回，无生产源码改动）

## 等待构成诊断（文献综合，未新增插桩）

22-span 的 `candidate_commit_prepare`（2.317s wall / 1.237s CPU）内约 1.08s "等待"从未被拆解。本轮结合既有三轮插桩数据（22-span、NPC 5-span、NPC 10-span）静态定位，无需新采样即得结论：

- `queue_npc_for_next_tick`（2.057s wall / 0.979s CPU）是 `prepare_candidate_commit` 的**内嵌调用**（candidate_commit.rs:65），两段的 1.08s 等待是同一个东西。
- 主体在 `capture_decision_snapshot`（1.765s wall / 0.696s CPU）：内部为两个**串行** par 相位——逐账户 experience 捕获（62,022 次，0.844s wall / 0.772s CPU）与 `capture_retail_analysis`（0.865s wall / 0.454s CPU，含 `DecisionChainObservation::capture_for_roots` 与 per-account 观察）。
- retail 观察读 `retail_experience` map 中**本 tick 刚安装**的新状态；experience par 相位的每账户产物恰是同一状态的 `Arc`。两相位状态域不相交（计划态 attention/watchlist/belief vs retail_experience），存在合并为单通道的等价改写空间。

## 假设与实现（完整 patch 见 e5-merged-channel-rollback.patch）

把两个 par 相位合并为一个 per-account 通道：计划态提前 `take`、观察根提前构建，同一 item 内先做 experience 捕获、再用刚产出的 `Arc` 内联执行该账户 retail 分析（`observe_retail_analysis`/`retail_cost_signal` 参数化 experience 来源）。首错序保持 experience 错误 → 安装错误 → retail 错误；两处 map 读点参数化后测试入口行为不变。

TDD：红灯测试证明 retail 分析与 peer experience 捕获重叠（旧串行布局必红）；1/4 worker 全字段等价 pin；experience 错误优先 pin。红灯测试初版有 50% 调度抖动（新版 rayon join 存在 steal 反转，账户 2 的 item 可能先于账户 1 启动），改为**双向有界闩锁**后 10/10 稳定——E2 式重叠测试都应采用双向闩锁。全部 16 组定向回归绿（capture 18、retail_analysis 10、decision_chain 80、auction 120、saved_runtime 37 等）；仅有的 2 个失败为已登记基线固有失败。

## 基准（顺序独占、同会话相邻交替配对，各 9 样本）

| pair | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| HEAD (30ce134d) | 219.32 | 233.44 | 220.21 | 222.57 | 227.55 | 231.45 | 227.44 | 232.29 | 228.44 |
| candidate (8e593b94) | 228.96 | 232.65 | 231.94 | 227.63 | 235.69 | 234.59 | 230.14 | 229.67 | 225.27 |

- HEAD 均值 226.97x、candidate 均值 230.73x（**+1.66%**）；配对差均值 +3.76x，但后两对翻负；区间明显重叠（candidate 最低 225.27 < HEAD 最高 233.44）；6/9 配对 candidate 胜。
- 判定：**低于 2% 门槛且区间不分离，撤回**。源码按 HEAD 原字节恢复（e5-merged-channel-rollback-proof.json），测试文件删除，debug/release 产物重新绑定 HEAD（probe 仍为 30ce134d 未动过）。

## 机制教训（比撤回本身更有价值）

- 每 tick 到期账户仅 ~20 个（62,022/3000），retail 子集更少且单条更肥。**少条目、延迟受限**的 par 相位合并后：单条 item 成本变为两段之和，轮次数不变，只省一次相位屏障——理论上限 ~1-1.5%，与实测 +1.66%±噪声吻合。E3 已有同型结论（"两相已真并行但被单一大股主导，逐股流水无解"）。
- **修正 22-span 的"等待"解读**：`candidate_commit`/`npc_queue` 的 wall−CPU 差不是可调度的空闲，而是少条目 par 相位的尾延迟（个别肥 item 决定 wall）。消除它只能减少单条 item 工作量或削减到期条目本身的工作，"更聪明的调度/合并"在此负载形态下没有空间。
- 后续方向：retail 单条分析的成本构成（~140µs/条的观察/信号计算）与 npc_queue stream 段（0.425s wall，串行回调），以及 join 外真正的串行段（`rebase_private_for_tick_commit`、事件装配）。

产物：e5-head-1..9 与 e5-cand-1..9 的 .json/.err/-cpu.json、rollback patch/proof、测试备份 e5-merged-channel-tests.rs.bak。
