# 当前状态与继续入口（2026-10-06）

## 当前目标与用户约束

- 工作区 `.worktree/engine-max-performance`，分支 `fix/engine-max-performance`。主工作区他人未提交内容未动。
- 16 核默认 20,007 NPC、5 股、MAX >=1000x，仍未达成。目标保持 active；不能降 NPC 数、跳 tick、删门禁或改单位凑速度。
- 最新要求先用真实 Rust server 控制变量找原因并提高倍速，再看 frontend；当前暂停 frontend 改动/浏览器测量。
- 每次 reviewer 必须新建，相关批次稳定后通常只审一次，有有效发现修复才由全新 reviewer 终审。Luna仅明确边界任务，复杂设计与根因主agent负责。

## 已提交与稳定批次

- 既有四次本地提交：d29df69b、44ed1f07、3b52f1c2、865eaa23；详见历史findings.md。累计 committed diff 已由全新 fresh_cumulative_review 静态通过，不重复补形式审。
- 当前稳定批次为 ledger分页/稀疏rebase/直接分页构建、caller NPC消费、protocol历史 frame Arc、玩家snapshot资源投影、新server actor探针，以及原有4项fixture失败/2项编译缺失修正。
- 证据与最新检查范围以 server-phase-findings.md 为准；新Sol独立复核 server-batch-review.md 无生产问题，唯一P3 serde测试证据缺口已经补强，另一全新Luna固定清单终审见 server-batch-final-review.md（未发布前不得宣称通过）。

## 主基准与当前数据

- 新 example `apps/server/examples/server_max_speed_probe.rs` 运行生产 SessionActor Fastest +真实广播，不含HTTP/WS网络序列化与frontend。
- release binary `.tmp/perf-target/release/examples/server_max_speed_probe`；独立 engine example仍叫max_speed_probe，两者不可再重名。
- config `agents/engine-max-performance/default-setup.json`：5股、20,000 retail+5inst+2hot、seed42、一个market tick=1秒。
- 固定16不同物理核affinity0–15、Rayon16、Tokio2、3000tick（含必要休市日推进），server从87.56x提高到100.81x，稳定复测103.19x；平均CPU约3.7核/16核预算23%。目标远未完成。
- 同配置纯engine清洁配对106.90x，server额外成本约3.5%；瓶颈主要在共享engine全量重复工作与细碎Rayon等待，继续以server作为主验收。
- 同16核affinity只改Rayon为4worker，server109.64x/3001ticks；没有利用全16核，不作为达标证据。
- CPU probe command：`timeout --signal=TERM --kill-after=1s 299s python3 agents/engine-max-performance/cpu-probe.py <label> 16 0-15 .tmp/perf-target/release/examples/server_max_speed_probe`。一次只跑一个基准，不能与本任务构建/其他性能基准重叠。产物SHA和逐秒/proc证据保存在ignored JSON。

## 检查与未解决事项

- engine ledger56、NPC21、snapshot5、protocol rollback28、hash14、continuous transaction19、auction transaction14、finalizer11、trade acceptance6、serverlib31、technical_memory33绿。filter有重叠，不能合计冒充唯一case总数。
- save_contract完整35/36绿；tampered_calendar_policy_is_rejected期待CivilClock(Calendar)，实际返回Calendar(DigestMismatch)。独立HEAD archive只补missing import后同样失败，保留原断言，未在本批修改错误传播。
- 原有conservation_rows_execute_on_multiple_rayon_workers出现一次快任务单worker赢完的调度概率失败；孤立case和4testthreads ledger全部重跑绿，日志保留，不宣称已修复flaky。
- 最新test target `.tmp/perf-tests`；单独engine lib binary engine-6ae5d5d49d140673，engine+server lib特征合并binary engine-3e4dbe3a7e208ca7/server-3d83fd07524ed41a。每次源码变更必须重新编译，不能凭文件名拿旧binary验收。
- 普通测试9秒TERM+1秒KILL硬限制；必要编译/长基准299秒TERM+1秒KILL。并发参数显式记录，不能把冷编译伪装测试耗时。
- WASM/frontend、HTTP/WS、全交易日/稳态和完整workspace验收尚未完成。旧Vite session74103不应凭记录假定live。

## 继续排查方向

- GDB自己启动baseline server子进程采样：server-stack-probe.py，baseline binary `.tmp/server-baseline-max-speed-probe`，60组原始栈server-stack-probe-samples.log。调试wall不作性能数据。
- 当前热点：SealAllocationSnapshot全表reservation投影，ledger守恒/evidence与live orders重复核对，逐股private ledger每tick重构，多个很短的Rayon屏障。稀疏COW只能降低复制，尚未消除验证的全量工作。
- 下一批主agent设计不可变page证据缓存/资源投影复用，以实际mutation使缓存失效；保留首错顺序/checked累计/全局守恒/失败原子性。资源校验先筛cache命中再派发失效页也是可测方向。
- 前端保持暂停；Rust server达到1000x之后，才恢复真实UI路径验证。
