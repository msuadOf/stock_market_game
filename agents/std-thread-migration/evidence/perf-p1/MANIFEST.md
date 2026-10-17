# 性能修复红证据清单（性能恢复战役·P1 批）

每份红日志 = 完整输出（行为红含 panic 原文；性能红含 left/right 数值口径）。
SHA=取证时 HEAD（工作树形态另行注明——本批遵循"测试+实现同工作树、
突变复核判别力"形态，突变内容在文件内注明）。

## P1-A：受理事实按组批发（world.rs 逐笔×全组 → owner 分桶批消息）

| 文件 | 内容 | 命令 | 红形态 | 绿形态 |
|---|---|---|---|---|
| p1a-red.txt | 突变红：AcceptedBatch 分支吞批（no-op 突变）→ 新守卫测试 panic"组 0 的受理事实必须下行（批发不丢条目）" | `cargo test -p engine --lib accepted_batch_keeps` | 工作树+突变（文件内注明基准 SHA） | 同命令完整态（p1a-green.txt） |
| p1a-green.txt | 守卫绿（新多组时序/定向测试+sol3_continuation）+全量回归双绿（1606/0/11×2）+flake 对照定性+A 探针 3 轮 KPI | 文件内逐段注明 | — | 本轮工作树 |
| ../perf-p0-diff-profile.md | 性能红（修复前基线）：t/s 71-105、4618 ctx-sw/tick、协调者 28 CPU·s、87% 内核周期——P1-A 的目标瓶颈实测 | perf record/stat（方法在文件内） | HEAD=7369cc96 | P1-A 后 KPI 见 p1a-green.txt 尾段 |

红证形态登记（如实）：P1-A 为**等价重构+性能修复**，行为面修复前后等价
（成交量/费用桶/冻结逐位一致）——行为红只能以突变形态证明新守卫的判别力；
性能红的 left/right=修复前后 KPI 数值（t/s、ctx-sw/tick、CPU·s）。

## P1-B：每 snapshot 共享 MarketView/attention signal（decision.rs+group.rs）

| 文件 | 内容 | 命令 | 红形态 | 绿形态 |
|---|---|---|---|---|
| p1b-red.txt | 突变红：observation 门判定取反 → golden 守卫 panic，left=[全 0 挂簿] right=[1,2,2,3,3] | `cargo test -p engine --lib shared_view_decision_sequence_golden` | 工作树+突变（基准 SHA 在文件内） | 同命令完整态 |
| p1b-green.txt | golden 守卫绿+全量回归双绿（1607/0/11×2）+A 探针 3 轮+B 口径两形态 | 文件内逐段注明 | — | 本轮工作树 |

等价登记（如实）：golden 守卫（单组确定性 fixture：cash/fees/volume/shares/decides/挂簿节奏逐字段）
锁定 base=0.3、seed=12345 形态；对"判定取反"突变判别力实证（p1b-red）。
"RNG 消费顺序对调"突变对该种子恰好鲁棒（draw 序列两序 observe 集合相同）——如实登记，
非测试缺陷（测试守卫的是决策序列输出，不是 RNG 内部顺序本身）。
大规模等价证据：A 探针 20000 NPC×3000 tick 终态逐位一致
（成交量 6438600/费用桶 647541185/冻结 6438600，修复前后相同）。

## P1-B 性能（A 口径，P1-A 基础上）
t/s：517.8（P1-A 后）→ 1220.9/1296.8/1242.8（mean 1253.5）≈ +142%
wall：5.62-5.98s → 2.31-2.46s；task-clock 9.7 → ~6.0 CPU·s
B 口径：groups=1 持平（4.14 vs 4.26 基线——单组无广播放大+链位不走共享路径，符合预期）；
groups=10 +12.5%（5.65 vs 5.02）。events 逐位一致（3336/3252）。

## P1-C：attention heap 无消费增长修复（failure.rs set_runtime_attention 镜像重建）

| 文件 | 内容 | 命令 | 红形态 | 绿形态 |
|---|---|---|---|---|
| p1c-red.txt | 红证（修复前）：heap=6 项 > 名册 1——"attention heap 不得随 tick 无界增长"panic | `ENGINE_STEP_RUNTIME=1 cargo test -p engine --lib runtime_attention_heap_mirror` | c7cd0eb7（修复前工作树） | 同命令完整态 |
| p1c-green.txt | heap 守卫绿+Sol 二审语义守卫绿+hash 回滚守卫绿+全量双绿（1608/0/11×2）+B 长跑对照 | 文件内注明 | — | 本轮工作树 |

修复形态登记（如实）：
- **set_runtime_attention 改为镜像重建**（消无界增长——heap 恒≤名册，每 tick O(N) 恒定）。
- **首次尝试"baseline 不 clone heap、回滚从权威重建"被 hash 守卫打回**：旧管线/混合路径的基线
  heap 可含陈旧项（实测 compute_context_shared_sessions：before 4 项含陈旧 vs 重建 2 项 →
  business_state_hash 红）。修正为 baseline 保留 heap 快照逐项还原（clone O(N) 恒定——
  无界增长已消，O(N·T²) 大头仍在消除范围）。
- B 口径长跑判据（协调者指定）：修复前 300→1000 tick 恶化 25%（4.26→3.18）；修复后不恶化
  （5.02→5.24），同 1000 tick 快 65%（314.9s→190.8s）。events/fees 两态逐位一致。

---

## 归档说明（2026-10-17 落库）

三提交与证据对应：
- accb6156（P1-A 批发）→ p1a-red/green.txt + perf-p0-diff-profile.md（性能红基线）
- 9a4dd018（P1-B 共享视图）→ p1b-red/green.txt
- 78e265e7（P1-C heap）→ p1c-red/green.txt

红证据形态：行为红=突变型（基准+突变内容在各文件头部注明，基准 SHA 为
取证时工作树 HEAD——突变无独立提交，不可由 SHA 单独复现，突变内容以
文件内注记为准）；性能红=修复前后 KPI 数值对比（perf-p0-diff-profile.md
与各 green 文件尾段）。原始 perf data（.tmp/perf-records/，1.3GB）不随
归档——方法与结论足以复现（命令在 P0 报告内）。

A 口径终值（P1 三件叠加，3000 tick/10 组×2000 NPC/5 股）：
1307.8/1391.6/1374.4 t/s（mean 1358，min 1308，max 1392）；
开局 71-105（19.2×），超 5412fd10 历史基线 663.7 达 2.05 倍。
B 口径：300 tick 5.02（+16-21%）、1000 tick 5.24（修复前 3.18）、
3000 tick 4.68。守恒：两口径全部通过（A：现金+费用+股份逐位等；
B：股份精确等+现金=初始−费用+events/fees 逐位一致）。
