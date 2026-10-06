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

## 追加：已撤回的 page proof 实验

- `5bc20409` 已本地提交且独立终审通过；此后探索过 whole/page conservation proof 与 complete evidence page proof。
- 所有实验正确性短测通过，但真实server相邻配对仅原版103.75x、candidate107.09x（约3.2%，单次样本），局部重验1024→32没有形成明显总吞吐收益。
- 为保持必要性和最小复杂度，已把本任务5个实验源码/测试文件逐字节回退到HEAD5bc20409；没有保留那约325行派生cache。证明proof-cache-rollback-proof.json，详细判定proof-cache-experiment.md，逆向实验源码proof-cache-rollback.patch。无需再审已撤回实验。
- 当前唯一未提交源码是 `packages/engine/examples/max_speed_probe.rs` 计时结束后的final_load诊断字段，不改生产代码；尚未做独立review，作为在途探针，不报告已完成批次。
- final-load-native-16实测：3835 live envelopes、14 plans、上一tick12retail decisions/3order diagnostics，约106.36x。phase耗时仍为decision/coordinator11.64s、SealAllocation3.61s、stock3.28s、receipt2.57s、precommit2.55s、expiry2.55s /3000tick。
- 最新test binary曾编译实验源码，虽然当前已回退生产文件，下一轮测试必须重新编译HEAD对应binary；不能拿带cache的test binary验收当前代码。
- Cargo共享target跨archive会受mtime影响：git archive旧mtime曾让Cargo错误复用candidate产物。已刷新archive输入mtime强制实际重编，核对SHA不同才配对；生产baseline独立binary `.tmp/server-proof-baseline-5bc20409`，candidate `.tmp/server-page-proof-candidate`。继续构建当前worktree时核对Cargo确实重编/产物指纹，不把Fresh当成源码同一的证明。
- 已读取本scope及父cgroup cpu.max均为max，nr_throttled=0；CPU空闲主要不能归因于配额限制。16worker实际只用约3.7核。
- 后续应沿实际订单扫描/逐股重复构建/细碎Rayon屏障定位更大成本；不能继续仅堆低收益校验cache。Rust server仍是主验收、frontend继续暂停、1000x目标active且未达成。


## 追加：当前 PrivateStockLedger 批次

上述“唯一未提交源码是探针”仅描述 page proof 撤回瞬间，当前已新增 private_stock_ledger.rs 并接线 Continuous worker。详细当前实现、128.45x 真实 server 证据、短测与撤回的无收益实验见 private-stock-ledger-findings.md。最新 profile：server-private-values-stack-samples.log，共48组864线程栈，全部16个Rayon worker和actor均采样；调试耗时不作性能证据。生产数学 helper 实验已回退，最新test binary为撤回后的实现。全新独立Sol reviewer已通过本批门禁，报告private-stock-ledger-independent-review.md；准备聚焦本地提交。最终server复测127.50x，与初次128.45x接近，目标远未达成。Rust server先达标、frontend暂停的顺序不变。


## 追加：迭代器长度实验已撤回

c06151ac已经本地提交并通过全新Sol独立审查，真实server约127–128x。后续CountedRows精确长度实验candidate126.33x、相邻已审查原版128.51x，无收益，源码已逐字节回退HEAD，详情ledger-row-count-experiment.md。主release路径及最新lib test binary暂时是已撤回实验artifact，不能直接验收；当前可用已审查独立binary .tmp/server-private-stock-c06151ac，SHA/配对见记录。下一轮先重新编译再验证源码变更。数学Copy helper也未保留。下一步应避免再追低收益微观迭代器，针对完整reservation/ledger重复投影和验证展开更大算法优化。


已审查原版额外Rayon1调度校准：119.186x，而Rayon16同配置128.505x，仅约7.82%的吞吐提升；这是刻意控制worker数的诊断，不称多核达标。详细ledger-row-count-experiment.md。接下来需针对caller/Rayon屏障与全量输入准备做更大成本分析，避免继续堆低收益cache/微优化。


## 追加：当前产物已重新绑定已审查源码

无收益CountedRows实验回退后，已用debug/release各-j8并行构建，总Cargo job预算16，独立target与299s deadline；两个编译均exit0（65s/54.22s）。当前主release server及lib test已回到c06151ac源码，不再含CountedRows或数学Copy helper。生产源码git diff为空。此前“主路径是实验artifact”只描述回退后的中间状态。最新1worker119.19x、16worker128.51x的校准使用独立已审查binary，不受这些构建重叠影响。


## 追加：LiveReservations在途批次

最新生产差异为不可变live ledger叶页的预留投影复用：LiveReservations共享Arc分页目录、Weak输入identity、只重算失效页，密集>=512行仍Rayon并行；AccountBook资源cache保留账户页/完整prices/实际预留依赖。子阶段诊断见substage-profile-findings.md，临时插桩已按18份原字节撤回，未进入生产模块。正式真实server无构建重叠相邻配对：c06151ac原版127.847x，新版139.665x，再次同SHA142.364x；约9%–11%收益，最近窗口92–97x，1000x仍远未完成。无效重叠/旧artifact样本已明确排除。

新LiveReservations8、account_book6、decision_resources11、ledger56、continuous101、NPC21、hash_contract14、phase_timing12、protocol rollback26及auction transaction14定向短测通过。filter有重叠，不累加唯一测试数；全部执行命令9秒TERM+1秒KILL、4 test threads。最新lib binary .tmp/perf-tests/debug/deps/engine-6ae5d5d49d140673 对应新增hash/serde边界测试，release对应同一生产实现但后续仅增cfg(test)；待本批最终构建时再绑定所有源码mtime。

全新独立Sol已静态通过生产批次，报告live-reservations-independent-review.md；4组非阻塞边界建议root已补核心路径，另一个全新一次性Luna固定核对新增4测试通过；部分可选组合未穷尽覆盖，详见boundary-final-review.md与findings记录，不宣称全部覆盖。准备聚焦本地提交。前端、WASM、完整workspace、HTTP/WS、全交易日与原save_contract失败仍未完成；不调用goal complete/blocked。


最终release source binding重新构建后正式复测142.419x /3000tick /21.0646s，SHA bf08dda9941d3b66a82cc4b457b652807eb1bed73a5e1eae17c3f463503fbbb5，最近窗口91.28x；全部构建/基准handle已exit0。完整生产Sol复核+新增测试固定Luna复核无阻塞；可选组合未穷尽覆盖如实登记。准备本地提交，不推送。后续继续优先真实Rust server >=1000，frontend暂停。子步骤显示Continuous输入准备2.78s、每tickhydrate2.41s和stock初始化2.26s仍有重复key/projection/clone工作；下批可评估重复校验事实的共同表示与复用，不能删checked或验证门禁。substage-profile-instrument.py只适配旧c06151ac的globalmap源码，不能直接在新的LiveReservations源码上重跑；计时产物保留并已撤回其全部插桩。
