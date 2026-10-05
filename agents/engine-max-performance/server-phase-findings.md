# Rust server 固定条件排查

用户要求先完成 Rust server 的 MAX 倍速优化，再回到前端。当前目标仍为 16 核默认场景至少 1000x；未达标。

## 固定负载与证据边界

- 使用默认 `SessionSetup`，5 股、20,000 retail + 5 inst + 2 hot、seed 42、2030-01-01 开局；一个市场 tick 为一秒。
- `server_max_speed_probe` 使用生产 `SessionManager` / `SessionActor`、Fastest（+Infinity）与真实 broadcast；连续核对每一已发布 tick。不是手工逐命令 step，也没有降低 NPC 数量。
- 使用 release、CPU affinity 0–15（同 NUMA、16 个不同物理核）、Rayon 16 worker、Tokio 2 worker。所有线程共同受这 16 核 affinity 限制；Tokio 另一 worker 为控制/订阅服务。
- 初始化另计；计时包含必要休市自然日推进。3000 tick 覆盖开盘竞价与连续竞价。这里包含 actor protocol 与广播订阅消费，不包含 HTTP/WS 序列化或前端渲染；不是端到端网页验收。
- 长基准使用外部 299s TERM / 1s KILL deadline，CPU probe 自身另有 295s deadline 与清理。JSON 保存 command、产物 SHA-256 和逐秒 CPU/线程/系统核占用；进程 CPU 不含其他任务。
- 最初 server example 曾与 engine example 同名导致二进制路径重合，已重命名 server example。前三个 server JSON 的 path 均核实为 `production_server_actor_fastest`；原始 server baseline binary 保存在 `.tmp/server-baseline-max-speed-probe`。

## 实测

| 版本 | 已提交 tick 倍速 | 3000 tick wall | 平均进程 CPU | 16 核预算利用率 |
|---|---:|---:|---:|---:|
| server-baseline-16 | 87.56x | 34.264s | 340.3% | 21.3% |
| server-thread-history-16 | 90.76x | 33.055s | 343.6% | 21.5% |
| server-snapshot-direct-ledger-16 | 100.81x | 29.760s | 366.2% | 22.9% |

第一批收益有限，仍远低于 1000x。配置线程数并不等于有效使用全部 CPU。

## 本批实现与 TDD

- 继承的 ledger 分页按 32 accounts/page、16 pages/group 保持完整 EnvelopeKey 排序；稀疏 rebase 保留未变化 page、跨 tick 累积费用与 receipt cursor。retain 删除末尾多行的 panic 已实际红测并修复。
- 直接构建三个 ledger 分页 map，消除先 BTreeMap 再分页的重复分配；保留重复 identity 拒绝和完整证据检查。
- 外部宿主 NPC stream 在 caller 消费结果，不为每 tick 新建 OS consumer；Rayon worker 内调用仍保留独立 consumer 避免 pool 饥饿。已有 fast/slow completion、Player 插入、失败后 drain、单 worker 和并发 session 测试全部保留。
- Protocol intraday 改为 `AppendOnlyHistory<Arc<TickFrame>>`，外层 checkpoint 触发不满 chunk COW 时只复制 Arc。新增 frame payload 地址与 rollback/retry 测试先红后绿；序列化与 CivilUpdate 对外仍为原 TickFrame 数组。
- 高频玩家 snapshot 使用借用订单、只投影玩家预留并直接查玩家账户。完整账户 snapshot 仍投影全部 NPC。新增玩家/全账户预留一致性及计算范围测试先红后绿；现金/股份溢出和部分成交累积费用测试保留。

## 回归异常与范围

- 连续竞价相关扩大回归暴露四个失败；在独立 HEAD archive 的基线源码重新编译、执行后，四项同样失败，证明不是本批引入。
- 两项 finalizer fixture 从休市日直接测试交易 tick，日 K 时间和预期交易日不一致；显式以 2030-01-02 交易日装配 fixture。
- 两项 acceptance fixture 默认开局资金已变为大额，但断言仍以 10000000 分为前提；只为该 fixture 显式设置原本期待的资金，不改任何断言。
- 前序已发现的两个 integration fixture 编译缺失（WithinKindDistribution import、PersonalHistoryReadLedger 变量）补齐，保持原断言。
- `server-paired-engine-16` 曾与 HEAD fixture 编译重叠，不能作为严格配对性能证据；仅保留诊断日志，正式配对需无构建/其他本任务基准重叠时重跑。

## 当前瓶颈与继续方向

- GDB 自己启动 server baseline 子进程采样（不附加他人进程），结果不作速度证据。60 组调用栈显示主路径大量 Rayon latch 等待、SealAllocationSnapshot、ledger 校验/投影、snapshot 与历史 chunk push。
- 连续竞价末段 actor 最近 0.5s 速度仅约 60–67x，3000 tick 平均掩盖了委托积累后的成本。需要增加阶段窗口，并进一步削减重复全量工作与细碎屏障。
- 不用忙等填 CPU，不跳 tick、不删守恒校验、不改交易时间/撮合优先级。下一批重点是原有事实的不可变 page 缓存与逐股 private ledger 重复构造。
- 当前停止前端测量与改动；后端达标后再验收前端真实 UI。

## 稳定批次复测与检查范围

- 关闭构建后顺序复测：真实 actor `server-fixed-16-rerun` 为 103.19x（29.072s / 3000 tick）；同配置纯 engine `server-paired-engine-clean-16` 为 106.90x（28.064s / 3000 tick）。主要瓶颈属于共享 engine，server 的 protocol/广播额外成本约 3.5%；仍以真实 actor 作为主验收入口。
- server lib 31、ledger 56、NPC 21、snapshot 5、protocol rollback 28、hash 14、continuous transaction 19、auction transaction 14、finalizer 11、trade acceptance 6、technical_memory 33 通过；相关 filter 可能重叠，不能直接求和作为唯一测试数。各执行命令均小于 10s。
- save_contract 完整套件 35/36 通过，`tampered_calendar_policy_is_rejected` 仍失败：fixture 期待 CivilClock(Calendar(...))，实际在 save policy 早期校验返回 Calendar(DigestMismatch)。独立 HEAD archive 只补缺失 import 后编译同套件，该 case 产生完全相同失败。本批未修改该错误传播契约或弱化断言，保留为已有未解决问题；不能宣称全量回归通过。
- 全新 Sol reviewer 的本轮报告为 server-batch-review.md：没有生产正确性或A股语义回归，唯一 P3 是原测试比较两个 JSON struct-key 序列化 Err。已改为成功的 canonical entry/hash 投影字节比较与明确 JSON map-key 错误契约，并保留 u64::MAX 排序/JS unsafe integer 拒绝，待读全新 reviewer 终审。前一 reviewer 不复用。
- WASM、完整 workspace 回归、HTTP/WS端到端、完整交易日与 frontend 尚未验收。本批局部绿灯与约100x不代表整个目标完成。

- 同一 16 核 affinity、同一 release binary、仅 Rayon worker 数变为 4，真实 server 得到 109.64x；这只证明当前细碎并行与串行瓶颈限制扩展，不能以少 worker 的配置宣称利用了全部 16 核。
- 修复 serde 测试后的 ledger filter 在 4 test threads 为 56/56 通过；一次 16 test threads 运行遇到原有 `conservation_rows_execute_on_multiple_rayon_workers` 概率失败（快任务被一个 worker 完成，>1 worker 的断言失败），孤立该 case 与4testthreads整filter重新执行通过。保留日志 server-review-fix-green.log / server-review-fix-green-stable.log，未隐藏或删除断言，尚未对其做调度稳定性修正。
