# Sol 战役复核四阻断修复计划（第 22 任，2026-10-19）

> 输入：Sol 复核四阻断（协调者转述）+ 台账勘误单。
> 纪律：红测试先行、一件一提交、显式 pathspec、实施批独立复核。

## P1-1 在途快照生命周期（session.rs:5228/5429、failure.rs:789/1521）

问题：runtime_resting_snapshot（P2-4 起 envelope 派生唯一现势源）在三处被清空
而 envelope_ledger 已停逐拍维护：shadow 克隆置 None / commit_from 置 None /
失败回滚置 None；另日结拍快照装于 Drain 前（清簿前在途残留）。
后果：checkpoint→rollback→driver 重建（resting_order_seeds_from_session 的
from_ledger 分支读 stale ledger）→ 新挂单消失/旧单复活/已转移现金股票失对应；
日结后信封查询/save 读清簿前订单。

修法（权威快照保留/移交/恢复）：
1. clone_for_shadow：snapshot 随 shadow 克隆（Arc 廉价）。
2. commit_from：移交 shadow 值（对齐 Sol 五轮 mirror/游标交接先例）。
3. RuntimeInstallBaseline 增 resting_snapshot（拍前值）；rollback 恢复拍前值
   （替代清空——失败拍后派生源=上一成功拍发布版本，与账户/镜像回滚面一致）。
4. 日结段 Drain 后重刷快照（live 现拉——发布版本仍是清簿前 tick）。
5. runtime_envelope_ledger_snapshot 快照感知：有快照→派生（重建驱动器种子
   读现势），无快照（restore 未步进）→实际 ledger。

红测试：
- T1 commit 交接：snapshot Some 经 shadow→commit 不丢（现红：None）。
- T2 回滚恢复：带在途前态失败回滚后派生投影==拍前在途集（现红：空）。
- T3 重建种子：回滚后 from_session_grouped 重建的 desk 种子含拍前在途单。
- T4 日终：日结拍后派生信封恒空（清簿）且 save 通过（现红：清簿前残留）。

## P1-2 清仓经历未入 belief 发布分片（group.rs:4114 早于 Seal apply_deferred）

问题：belief 稀疏分片随 GroupReport（决策相）上行，早于 Seal 的
apply_deferred（清仓经历转移）；main 的同拍补偿 runtime_record_full_exit
已随 D3 退役删除。终轮清仓后 session/发布 belief 仍留持仓经历，save 可触发
fault2051；日结拍 DayEnd 清 dirty 使该拍经历永久丢失。

修法（分片随 SealAck——组权威单源）：
1. SealAck 增 belief_sparse；Seal 处理在 apply_deferred 后构建（构建逻辑
   自 run_round 提取共用 fn）。
2. GroupReport 退 belief_sparse；converge_round/TickEvidence.group_belief 退役；
   seal_round 收集双分片；install_tick 自 SealAck 消费。
3. 四面（组权威/发布/session/save-restore）同拍一致由单源构造承载。

红测试：外部意图终轮清仓（p2_4b fixture 式 NPC 意图注入）后：
- 组权威行=已清仓经历（entry None+cooldown 在+last_sell 有）；
- 发布分片行、session retail_experience 行==组权威行（现红：仍持仓形态）；
- save() 不触发 fault2051（现红）且 restore 往返保持。

## P1-3 真实 Seal 超时违反存活合同（world.rs:1768、group.rs:5616）

问题：seal_round 广播后超时→abort_tick 广播 Abort；慢组队列 [Seal,Abort]，
Seal drain 把 Abort 判协议错位退出（worker 死=违反存活合同）；迟到 SealAck
污染下一轮收束（converge 期望 GroupReport 收到 SealAck→Disconnected）。
现有注入在广播前，避开真实路径。

修法：
1. 组 Seal drain：Abort=中止信号（rollback+跳过 SealAck+存活回主循环）。
2. seal_ack_failure_once 注入移到广播后（首个等待点合成超时；consume_first
   参数支持"部分 Ack 后"超时）——注入面走真实路径。
3. seal_round 记已 Ack 组集：重复 Ack（迟到者）跳过不计。
4. converge_round：迟到 SealAck（已判死尝试的合法竞态产物）跳过不报错。

红测试：
- 既有存活契约测试升级为真实路径（广播后超时+部分 Ack）——现红：
  慢组 Failed 退出/迟到 Ack 污染重试收束。
- converge_round 单测：预置迟到 SealAck 后正常收束。

## P2-4 无快照时业务 hash 漏真实 ledger（hash.rs:213、failure.rs:2277）

问题：business_state_hash 无条件派生（无快照=空集）——旧管线（ledger 维护中）、
restore 未步进、shadow 非空 ledger 均不入 hash。

修法：统一"有快照派生、无快照读实际 ledger"合同（对齐 save 面
saved_runtime.rs:123 先例）。

红测试：①runtime 存档→restore 后（未步进）business_state_hash==存档前值
（现红：restore 侧派生空集）；②旧管线非空 ledger 内容变化→hash 变化。

## 台账勘误（final-ledger.md）

- A 基线 1332.6 非 544.7（544.7 是 B）；
- D1 "全退役"→EchoPlans 生产往返仍在（16.6μs ⑤刀登记）；
- D5 补终态；events 分片=默认占位非"已完成"；
- "零会话层税"改"方向"；R19 三项登记补处置结果。

## 收尾

全量回归（默认+ENGINE_STEP_RUNTIME=1+wasm）→独立复核（未实施 subagent）
→报新冻结点。
