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

## P2-1a：个人态 dirty 掩码（EchoBelief 附 dirty+驱动器只装演进位）

| 文件 | 内容 | 命令 | 红形态 | 绿形态 |
|---|---|---|---|---|
| p2-1a-red.txt | 突变红：dirty 过滤移除（全量重装）→ 守卫 panic left=8 right=0 | `cargo test -p engine --lib p2_1a_personal` | 工作树+突变（基准 SHA 在文件内） | 同命令完整态 |
| p2-1a-green.txt | 守卫绿+同源/注入读面绿+全量双绿+B 口径 5 轮+1000 tick 等价 | 文件内注明 | — | 本轮工作树 |

机制：组端三个生产演进面（retail 写回/chain 写回/信息投递）+测试注入标记
belief_dirty；Start/DayEnd 随 undo 清；EchoBelief 全量 states 保留（读面
同源校验）+附 dirty 掩码；驱动器安装循环只装 dirty 位。等价不变式：
非 dirty ⇒ 组端值==session 镜像值（构造期同源投影+两侧只经 dirty 通道同步）。
B 口径：5.02→≈10.1（+101%）；深拷贝安装链（≈58% CPU）大头消除。

## P2-1b / P2-2a：账户回写 dirty / EchoBelief 稀疏上行+增量缓存

| 文件 | 内容 | 命令 | 红形态 | 绿形态 |
|---|---|---|---|---|
| p2-1b-red.txt | 突变红：回写 dirty 过滤移除→left=7 right=0 | `cargo test -p engine --lib p2_1b_account` | 工作树+突变 | 同命令完整态 |
| p2-1b-green | 守卫绿+全量双绿+B 300 三轮（10.1-10.7）+1000（11.2） | — | — | 本轮 |
| p2-2a-red.txt | 突变红：组侧稀疏过滤恒真（全量上行）→installs=8 红稀疏判别 | `cargo test -p engine --lib p2_1a_personal` | 工作树+突变 | 同命令完整态 |
| p2-2a-green.txt | 三守卫绿+全量双绿+B 65.6 mean+1000 tick 53.0 | 文件内注明 | — | 本轮 |

P2-1b 机制：驱动器侧事实 dirty（trades/lifecycle/receipts owner 两拍并集——
组端 staging 落账滞后一拍的整值同步在下一拍；日结后强制全量一次覆盖
Drain/T+1/AdminBatch 无事实事件面）。收益小（300 tick +3%、1000 +10%）——
大头在组侧上行。
P2-2a 机制：EchoBelief 组侧稀疏（只 clone dirty 且在册条目）+驱动器增量
全量缓存（构造期=belief_seeds 同源，collect_belief 读面语义不变）。注入
窗口语义修复：InstallPersonal 的 dirty 由跨 Start 的 pending 集承载到
上行消费（对齐 pending_force_due 模式——d3b5 守卫逼出）。
B 口径：10.4→65.6（+530%）——组线程全量 clone+缺页税消除。

## P2-2b：安装基线账户域写时 undo（1cb87aab）

| 文件 | 内容 | 命令 | 红形态 | 绿形态 |
|---|---|---|---|---|
| p2-2b-red.txt | 突变红：记录 no-op→结构判别 left=0/right=2 | `cargo test -p engine --lib p2_2b_account_undo` | 工作树+突变 | 同命令完整态 |
| p2-2b-green.txt | 守卫绿+hash 守卫绿+全量双绿+B 78.1 mean+1000 tick 69.5 | 文件内注明 | — | 本轮 |

机制：baseline accounts 全名册快照退役；drive 回写/同拍投影两写入点
写前记录首写前值（幂等）；失败回滚消费、成功尾清。B 口径 65.6→78.1
（+19%）；1000 tick 69.5（修复前 3.18，累计 21.9×）。
