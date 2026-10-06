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


## 追加：借用Envelope投影批次（2026-10-06）

共享payload与仅charged setter两实验均无显著收益，已保存patch/hash并完整撤回。当前稳定候选以BorrowedTickStartEnvelope借用身份/Copy审计验证已有ledger，避免整批owned Envelope/String构建；empty hydration与公开owned API保持原实现，adapter复用同一费用数学facts，lookup key按股复用。全部资源/费用/字段/map-key/terminal/首错与失败原子性门禁保留。详见borrowed-envelope-findings.md。最终source binding真实server153.147x/155.449x（同SHA5a51e9122c9bf1c6e13431bd8bcaad28b18dae0cf484068d983d543642858306），相邻HEAD139.278x；此前候选148–149x也保留，目标1000x仍远未达成，CPU约4.31核，未满16核。独立frozen candidate .tmp/server-borrowed-projection-frozen-candidate。

全新Sol完整diff独立复核通过、无阻塞；可选混合错误首错fixture未穷尽登记。TDD两个allocation红绿与逐字段Eq/owned门禁精确Result边界通过；宽continuous命令曾9s超时，之后已拆独立套件固定RAYON4四进程并行，每命令<=3.2s。最终hyd11/borrowed2/adapter10/ledger_state25/hash14/expiry12/stock execution6均通过，filter有重叠不累加；ledger_tests filter匹配0不算证据。所有本轮build/benchmark handle已exit0。frontend/WASM/HTTPWS/full workspace/full day未验收；既有save_contract未改。后续继续先优化Rust server，准备更新热点诊断，避免继续追微小迭代器/缓存收益。不调用goal complete/blocked。


## 追加：借用Envelope投影批次（2026-10-06）

共享payload与仅charged setter两实验均无显著收益，已保存patch/hash并完整撤回。当前稳定候选以BorrowedTickStartEnvelope借用身份/Copy审计验证已有ledger，避免整批owned Envelope/String构建；empty hydration与公开owned API保持原实现，adapter复用同一费用数学facts，lookup key按股复用。全部资源/费用/字段/map-key/terminal/首错与失败原子性门禁保留。详见borrowed-envelope-findings.md。最终source binding真实server153.147x/155.449x（同SHA5a51e9122c9bf1c6e13431bd8bcaad28b18dae0cf484068d983d543642858306），相邻HEAD139.278x；此前候选148–149x也保留，目标1000x仍远未达成，CPU约4.31核，未满16核。独立frozen candidate .tmp/server-borrowed-projection-frozen-candidate。

全新Sol完整diff独立复核通过、无阻塞；可选混合错误首错fixture未穷尽登记。TDD两个allocation红绿与逐字段Eq/owned门禁精确Result边界通过；宽continuous命令曾9s超时，之后已拆独立套件固定RAYON4四进程并行，每命令<=3.2s。最终hyd11/borrowed2/adapter10/ledger_state25/hash14/expiry12/stock execution6均通过，filter有重叠不累加；ledger_tests filter匹配0不算证据。所有本轮build/benchmark handle已exit0。frontend/WASM/HTTPWS/full workspace/full day未验收；既有save_contract未改。后续继续先优化Rust server，准备更新热点诊断，避免继续追微小迭代器/缓存收益。不调用goal complete/blocked。


## 追加：当前有序联合实验已撤回

最新已审查提交f0ac8317的真实server155.45x/153.15x，独立binary .tmp/server-borrowed-projection-frozen-candidate。其后29子阶段profile已22文件原字节撤回，诊断substage-profile-f0ac-findings.md；GDB48组864线程栈见borrowed-stack-findings.md。最新有序联合ledger实验虽然point lookups2565→0且短测绿色，却真实candidate139.45x、相邻HEAD154.28x，故已4文件原字节恢复；详见ordered-evidence-findings.md。不继续堆无收益cache/局部扫描，也不把少查询当成已优化。原型未审/未提交；下一步需检查并行caller准备与真实资源瓶颈。正在重新编译HEAD release/test恢复artifact绑定。1000x仍未完成、frontend暂停，goal保持active。

恢复构建已确认两个handle exit0：debug37.17s、release54.96s，各-j8/总16job/299s deadline。当前主release SHA与独立已审查f0ac frozen binary逐字一致5a51e9122c9bf1c6e13431bd8bcaad28b18dae0cf484068d983d543642858306；主test也已重编HEAD。恢复后ledger57/hydration11/adapter10/hash14四进程RAYON4/test4短测通过，无已知在途任务。当前唯一tracked diff为本工作记录追加，生产diff为空。


## 追加：rebase完整构造证明批次

已审查f0ac8317上发现rebase成功后input门禁重复扫描；完整输入校验和逐变化Envelope.validate保留，成功构造输出后向既有Weak四map EvidenceCache保留证明，后续mutation照常失效。两文件窄diff（ledger生产+19/-6、测试3case）全新一次性Luna high独立审核通过。ledger60/hash14/continuous tick19/auction transaction14短测通过；真实candidate163.98x、同SHA确认164.41x，相邻原版155.04x，CPU约4.22核，1000x仍未达成。frozen binary .tmp/server-rebase-proof-frozen-candidate，SHAa7a2b34cf8eccdb49eb11786e316d0c9385ba3da623aff55e9f71f6f4fc30c31。详情rebase-evidence-proof-findings.md/rebase-proof-independent-review.md。所有handles exit0，frontend暂停；准备聚焦本地提交。下轮可实测已有OrderId identity索引避免cancel的完整簿find，以及expiry生命周期的完整ledger find；不能只凭更少lookup就认定收益。


## 追加：正式 server Cargo feature 基准纠正

rebase成功证明已经本地提交10f8d9ae。随后核对Cargo artifact确认旧apps/server example因dev-dependency打开engine verification-harness/Tokio test-util：真实actor路径是真的，编译配置含验证工具，不能冒充普通server配置。已建立agents/engine-max-performance/production-server-probe独立普通binary，main.rs与旧探针逐字节相同、依赖版本/source/checksum完全沿用root Cargo.lock，actual compiler artifact engine[]/server[]/Tokio无test-util；verify-build.py阻止旧源码/feature/依赖版本混用。正式baseline两次159.72x/163.22x，SHA5b3d306aa910c778b701f43d3c81663375108c77d9152b2f8eb2700eb9f3ad5e，独立binary .tmp/server-production-normal-10f8d9ae。旧带harness的164x及1/4/32worker诊断如实保留，不混作正式binary基准。首次独立manifest升级transitive的构建已废弃、从未测量；重新复制root lock并--locked构建exit0后才测。全新Luna限定工具review通过，无阻塞；可选executable target-dir限制未补。详细production-build-feature-findings.md/production-probe-independent-review.md。今后真实server主验收走普通binary、保留3000tick与CPU记录，不再用dev example配置替代。1000x依旧active、frontend继续暂停。


## 追加：本轮当前稳定状态与已否决的 coordinator 实验

最新生产优化提交10f8d9ae（rebase证明）、基准配置工具提交0e139d7b。正式非harness普通binary才是后续主验收：engine/server features=[]、Tokio无test-util、依赖root lock原版本，固定16物理核0–15/Rayon16/Tokio2/default5股20007NPC seed42。三次正式基线159.72x/163.22x/163.23x，最近窗口107–110x；CPU约4.2–4.3核。SHA5b3d306aa910c778b701f43d3c81663375108c77d9152b2f8eb2700eb9f3ad5e，独立binary .tmp/server-production-normal-10f8d9ae，主构建路径 .tmp/production-probe-target/release/production-server-max-probe。正式普通bin构建与verifier流程见production-server-probe/README.md。旧example164x属于harness配置，不再混为正式binary。

曾试server整step进入globalRayon pool，TDD线程上下文/精确Result红绿、fatal11/interval6绿色，candidate161.89x vs相邻HEAD163.23x无收益，已四份source/manifest/lock原字节恢复并删除本实验新helper测试（已归档）。原型未审未提交不补形式review，证据coordinator-pool-experiment.md/coordinator-pool-rollback-proof.json。现在正在重编普通bin/server lib test，完成后须确认两个handle exit0及SHA回到稳定baseline；不能用实验artifact验收。

本轮属于实际progress：生产state改变（10f8d9ae）并修正Cargo测量配置（0e139d7b）；目标1000x仍未完成、frontend暂停。后续避免继续仅尝试减少worker/切换caller/全局有序join：已测4worker173.5x与32worker133.3x（harness诊断）说明线程增加不等于更快，整个tick进pool已正式配置否决。较大剩余重复成本在每tick全量hydrate/input/snapshot/ledger/private book验证；可设计共用不可变page粒度事实/证据以减少重建与重复校验，但必须保留资源checked/首错/费用/原子性并用正式bin控制测量。不能恢复旧page proof/SharedRows<Envelope>等无收益复杂原型。

恢复构建两个handle已确认exit0（server test8.98s、正式probe23.32s，debug/release各-j8总16job/299s deadline）。普通probe verifier全部通过，主binary SHA与已审查独立baseline逐字相同5b3d306aa910c778b701f43d3c81663375108c77d9152b2f8eb2700eb9f3ad5e；最新server lib test .tmp/server-tests/debug/deps/server-02e6531ddd7290a4。恢复后fatal11/interval6短测绿色。无已知在途任务，唯一tracked diff为本工作记录追加，生产源码/manifest/lock diff为空。


## 追加：coarse snapshot撤回与连续结果缓冲区候选

整批snapshot/cache/sparse private ledger实验四次相邻只有约2%改善，15个tracked源码已逐字节恢复HEAD，新模块已归档并移出生产路径。详情coarse-snapshot-findings.md/rollback.patch/rollback-proof.json。未复核/未提交已撤回生产实验。当前候选只改ledger_conservation::map_rows收集布局，全部门禁保留；独占真实普通server168.515x/168.227x，相邻baseline161.531x，约4.1–4.3%改善。首170.073x样本与短测重叠已排除。详情flat-row-findings.md。debug/release各-j8重新编译并确认两个handle exit0，release feature/lock verifier通过；ledger_conservation6/ledger_state28/private6/continuous tick19短测通过。新建一次性Luna限定独立review在途，尚未提交。目标1000x仍未达，CPU约4核，frontend继续暂停。


连续结果缓冲区已通过新建Luna high限定独立复核（无阻塞），本地提交35bbfbd6，仅1文件+5/-5。当前稳定普通binary .tmp/server-flat-row-frozen-candidate，SHA7a6b1ac2b1da647476383ffb2b1e012078b9ab8585d82f49d4fbb3e60dd7fb6e；主正式binary同SHA。最新GDB启动本任务子进程48组/864线程栈已exit0；perf受perf_event_paranoid4拒绝，未修改系统。栈原始文件server-production-flat-row-stack-samples.log，仅用于等待/热点诊断，不作吞吐证据。下一实验只合并完整账本门禁的两轮并行映射，保留原序gate/checked汇总；目前仅新增成本红灯测试和cfg(test)计数，在途编译，尚无候选生产实现。


## 追加：当前可提交的完整门禁共同映射

当前diff仅ledger.rs/ledger_conservation.rs/ledger_state_tests.rs，dense门禁只映射一次，结构错误延迟到原守恒/checked全局累加、资源、基数门禁之后选首错，小输入仍原行为。两成本/组合用例红绿/边界通过；state30/conservation6/continuous tick19/auction tick14/receipt20/envelope9/hash14短测全部绿，匹配0的错误auction filter不计。最终debug21.59s/release53.88s（各-j8，总16job）均exit0，普通server feature/lock/source verifier通过。最终同SHA59ee89cd9dd0629f75ae534495dba31223229dacfb116900f1929c1327267567测169.976x/173.919x，相邻独立稳定35bbfbd6测168.169x，约1.1–3.4%且有波动；不能混用旧原型SHA172.5–172.8x。CPU约4核，未达1000x/满16核。全新一次性Luna high完整三文件diff独立复核无阻塞，报告combined-evidence-independent-review.md。准备聚焦本地提交；frontend/WASM/HTTPWS/full workspace/full day仍未验收。


共同映射批次已本地提交 **0ed22904**，上一连续结果提交 **35bbfbd6**；当前生产源码/manifest/lock无未提交diff，唯一tracked任务记录仍为本文件追加。正式普通binary主路径与冻结候选同SHA59ee89cd9dd0629f75ae534495dba31223229dacfb116900f1929c1327267567，已绑定最终源码。所有本轮build/benchmark/GDB handle均终态exit0（首次红灯/编译失败/沙箱GDB失败如前述日志单列），无已知在途长任务。两个独立Luna reviewer均新建且已结束，未复用。未推送，主工作区未改。目标仍active，1000x未达，继续Rust server优先；不要仅凭这次约170–174x或实际4.4核宣告目标完成。

下一轮需转向更大的完整tick成本：已有每股输入准备/owned snapshot/stock initialization以及最终private book/ledger验证仍每tick遍历全部live order；全局分页三map点查询和短Rayon屏障也仍存在。先采样/计时确认实际CPU成本与逐阶段caller等待，不重新恢复已否决的全局join、粗粒度snapshot cache或pageproof原型。StockCode仍是拥有String的newtype，批量key clone存在分配但目前没有精确allocation profile，不应仅凭该事实做大范围Arc/内联字符串API迁移；若追此方向需先证实全局成本，并保留所有字符串值、排序、wire/hash和异常输入语义。perf被系统配置拒绝且未修改设置，可继续使用仅针对本任务子进程的GDB或可完整撤回的阶段插桩。frontend/WASM/HTTPWS/fullworkspace/full day及既有save_contract错误仍未处理。


## 追加：股内行校验并行批次已提交（2026-10-06）

最新稳定本地提交 **68db9414**，默认5股/20007NPC/seed42/Rayon16/Tokio2/16物理核0–15普通真实server。最终同SHA **a643e60b516d207a8652b16ea9c58c70cfd0accf91fca6d16f9580b2f6d95c4f**，主路径 .tmp/production-probe-target/release/production-server-max-probe 与冻结 .tmp/server-stock-row-parallel-final-candidate 一致。正式最终190.625x /192.653x，相邻独立旧版0ed22904为171.314x，约11.3%/12.5%；CPU约5.4核，最近半秒141–144x。1000x仍未达，不调用goal complete/blocked，前端继续暂停。

本批只改continuous adapter/private market gate两处>=512独立行用Rayon with_min_len128/map_init并行，小输入保持原循环；所有Envelope/origin/audit/fee/resource/count与checked数学/首错门禁保留。adapter按book原序预标duplicate但错误仍本行原位置选；Result保序收集，不改变撮合或受理顺序。两1024行真实多worker红绿测试+511/512/513×1/4worker反序OrderId首错与失败无权威mutation测试通过；adapter12/matching29/private6/state30/continuous tick19/auction tick14/hash14/stock execution6短测绿色，普通命令9s TERM+1s KILL。debug/release各-j8总16job最终14.40s/53.81s，确认exit0及probe feature/lock/source verifier通过后才顺序独占基准；最终仅去除多重借用与调整import，不混旧原型SHA的188–191.6x。新建一次性Luna high完整四文件独立复核无阻塞，已结束。详情stock-row-parallel-findings.md/stock-row-parallel-independent-review.md。

本轮两个临时profile原字节均撤回27份源码并移出新增module，诊断substage-profile-0ed-findings.md/fields-findings.md；不要用插桩binary做吞吐。thread CPU/allocator wrapper统计当前同步span线程，不包含其他child worker，parent span重叠不可相加；realloc计新分配+旧释放，不等于RSS。字段clone采样：npc_order_lifecycles11.32m allocations但CPU0.222s，attention_scheduler请求2162MiB但CPU0.136s，ingress cursors0.211s；未优先做Arc迁移。一次性Luna固定三字段类型/mutation清单已结束，未参与实现。编译前途曾无Default/Market无Serialize红build已修，再取得真实行为红灯，没用编译失败冒充TDD。

下一步针对其他已量到的全量构造：continuous initial snapshot validation会复制完整resting_orders并构造OrderId BTreeMap（旧profile约1.2CPU秒/956k alloc/1497MiB）；可以小实验评估只用于按id查找/remove、无迭代输出的临时索引布局（比如保留完整validation顺序与对外Result的HashMap或紧凑有序索引），必须验证duplicate/missing/extra/cross-owner/stock/audit首错，不能因随机hash迭代改变输出。尚无该实验源码，不要把下一方向当已实现。当前生产/manifest/lock无diff，唯一tracked current-state.md追加，无在途长任务；主工作区未改，未推送。


## 2026-10-06 后端控制变量续排查：全部本轮原型已撤回

仍以稳定提交 **68db9414** 为生产实现；普通 Rust server、默认5股/20007NPC/seed42、16物理核0–15、Rayon16/Tokio2。1000x目标保持active，frontend继续暂停，不宣告完成。

本轮按价位COW实验189.2985x，对照189.9049x；补准确size_hint后185.0682x，无收益，完整撤回两tracked源码和未跟踪price_rows模块，归档完整patch/hash/module，见price-level-cow-findings.md。新迭代长度测试真实失败(0,None)对(4,Some(4))，修正后通过；普通短测保持9s TERM+1s KILL。

稳定冻结同一SHA的worker数控制实验，均固定affinity0–15、Tokio2、3000ticks，顺序独占不重叠：1worker169.3808x/CPU1.079核，4worker200.3265x/2.190核，8worker204.6854x/3.202核，16worker191.0120x/5.331核。CPU均值排除结束时线程消失窗口，不是runnable数；单次worker扩展回退不当统计结论，不改变生产worker配置。证据stable-worker-scaling-findings.md。一次性新Luna high只读检查四个并行段，未改文件或自行找任务，非独立复核角色；指出账户页粒度与股内/守恒嵌套扇出为待测假说。

账户页with_min_len8仅一行调度实验191.6495x对191.0120x（约0.33%），短测账户6/resources11/continuous tick19/auction tick14绿，撤回，无独立复核或生产提交。到达序排序消除实验保持同seq稳定首错和所有费用/ledger门禁，新增两个回归测试及hydration6绿；候选185.3714x对183.5473x（约0.99%），无明确收益，完整撤回，未独立复核/提交。两者是性能重构实验，没有编造功能红灯。记录account-page-grain-findings.md / arrival-sort-findings.md及patch/hash。

新的68d阶段诊断未改allocator，36span、仅wall/当前同步线程CPU；-j16构建51.47s exit0，feature/lock/source verifier通过，独占诊断exit0。private market gate wall约0.919s，stock input2.254s，continuous initialization1.916s，live projection1.762s，NPC snapshot1.858s/3000ticks，parent overlap不得相加，CPU不含其他child worker。27源码完整按保存字节撤回，新增module移出生产路径；substage-profile-68d-findings.md。

所有实验撤回后debug38.65s/release构建确认exit0，各-j8共16job/299s外部deadline；普通probe verifier通过，主release与冻结稳定binary SHA比较见restored-68d-build-binding.json。生产源码/manifest/lock无diff；唯一tracked仍current-state.md历史追加，没有已知在途长任务。未改主工作区、未推送，WASM/真实frontend/HTTPWS/fullworkspace/全天验收仍未完成，save_contract旧日历失败未改。

下一步优先查调度额外CPU：同一binary的16worker CPU消费约5.33核而1worker约1.08核，吞吐只改善169→191x；记录user/system CPU、worker runnable/wchan和临界段有效工作，区分Rayon spin/wake、任务拆分与真实业务计算，不能把这几个CPU统计直接当成已证明的根因。再考虑最小范围提高粒度/减少重复准备；不要恢复无收益的价位COW、账户grain8、arrival排序消除、旧page/整股缓存或全tick入池实验。

## 追加：read-only Continuous start 并行准备（2026-10-06）

当前未提交批次新增 `session/pipeline/read_only_start.rs` 并接线 `plan_tick`：当且仅当 `Continuous`、live ledger 非空且不存在 `expires_market_minute <= current` 时，用嵌套 `rayon::join` 只读并行准备 live projection hydration、`DecisionResourceSnapshot::seal` 与 incremental stock inputs；空 ledger、到期报价和其他交易阶段完整回退原 expiry/allocation 顺序。stock input 错误保留为 `Result`，延迟到原 DecisionShadow 入口/context 校验之后传播；hydration/resources 仍按原优先级消费。`envelope_projection.rs` 只为测试暴露逐行 observer，`docs/architecture.md` 登记契约，相关测试覆盖 eligibility、并行交错、1/4 worker 全字段一致、混合错误优先级与 authority hash 不变。

最终 review 修复后 release 构建绑定 `read-only-start-review-fix-production-build-binding.json`，verify-build 全部通过，binary SHA `eac5e6f2fa225cf0944c757cd31e1097ff3a08f02d7acd010485da968d10b796`。定向短测 read_only_start 5、continuous transaction 19、quote expiry 12、decision resources 11、phase timing 12、hash contract 14、auction transaction 14 全绿；每条命令 9s TERM + 1s KILL，RAYON4/test4 并行。四条正式 3000 tick benchmark 顺序独占：candidate 222.757x/216.501x，相邻稳定 68db9414 baseline 189.914x/189.614x；均值 219.629x 对 189.764x，提升 15.74%，candidate CPU 约 5.98 核。详见 `read-only-start-final-benchmarks.md`。

复核：第一次 review 的有效发现已修复并重新构建/复测；最终全新一次性 gpt-6-luna 只读 reviewer（Codex task `01a10f91-e405-7c51-85d9-1c1aebeda3ed`）结论 PASS、无阻塞语义问题，记录 `read-only-start-final-review.md`。此前 4 次新建 collaboration reviewer 均因平台 `thinking_signature_invalid` 未开始，未复用任何 reviewer。本批准备本地提交 `perf(engine): parallelize read-only continuous start preparation`；1000x 仍未达成，frontend/WASM/HTTP/WS/full workspace/全天验收继续暂停，不调用 goal complete/blocked。

## 追加：read-only start 后的继续排查（2026-10-06）

已提交 `382c1871` 后继续静态审查：`runtime_snapshot`/`tick_batch_delta` 每 tick 各扫描全部 resting orders 但只消费玩家预留；protocol facts 至少在构造和 `Frame.validate` 两次 canonical JSON；`ProtocolSession::step_frame` 为 intraday 额外完整克隆 frame；NPC next-tick queue 在 candidate commit 尾部串行。静态分析见 `read-only-start-post-batch-analysis.md`。

完成一个可撤回小实验：把已提交 frame 与 intraday history 共享同一 `Arc`，公开 owned API 保持一次返回克隆。protocol 47 测绿，candidate 223.040x/217.658x，相邻 382c1871 220.021x/223.969x，均值约 -0.74%，波动内无收益；源码已逐字节回退，详情 `frame-clone-count-findings.md`。主 `.tmp/production-probe-target` release 现在是已撤回实验 SHA `c9b1526aa40f...`，不能代表当前 HEAD；当前稳定 frozen binary 仍为 `.tmp/server-read-only-start-final-candidate`，SHA `eac5e6f2fa225cf0944c757cd31e1097ff3a08f02d7acd010485da968d10b796`。1000x 未达成，frontend 继续暂停。

## 追加：read-only start 后的继续排查（2026-10-06）

已提交 `382c1871` 后继续静态审查：`runtime_snapshot`/`tick_batch_delta` 每 tick 各扫描全部 resting orders 但只消费玩家预留；protocol facts 至少在构造和 `Frame.validate` 两次 canonical JSON；`ProtocolSession::step_frame` 为 intraday 额外完整克隆 frame；NPC next-tick queue 在 candidate commit 尾部串行。静态分析见 `read-only-start-post-batch-analysis.md`。

完成一个可撤回小实验：把已提交 frame 与 intraday history 共享同一 `Arc`，公开 owned API 保持一次返回克隆。protocol 47 测绿，candidate 223.040x/217.658x，相邻 382c1871 220.021x/223.969x，均值约 -0.74%，波动内无收益；源码已逐字节回退，详情 `frame-clone-count-findings.md`。主 `.tmp/production-probe-target` release 现在是已撤回实验 SHA `c9b1526aa40f...`，不能代表当前 HEAD；当前稳定 frozen binary 仍为 `.tmp/server-read-only-start-final-candidate`，SHA `eac5e6f2fa225cf0944c757cd31e1097ff3a08f02d7acd010485da968d10b796`。1000x 未达成，frontend 继续暂停。

## 追加：当前 critical path 插桩结果（2026-10-06）

对已提交 `382c1871` 源码做 22-span 临时插桩并独占采样（诊断 218.378x，不作正式提速）：协议/发布外层成本合计小（checkpoint 0.355s、frame after step 0.161s、frame validate 0.150s、snapshot runtime 0.107s、tick batch delta 0.102s、prepare frame 0.088s/3000 tick）；`runtime_snapshot` 的 NPC order 全量扫描与重复 canonical 静态担忧实测只有约 0.1–0.15s，暂不优先改。`npc_queue_next_tick` wall 2.057s/CPU 0.979s，`npc_snapshot_capture` wall 1.765s/CPU 0.696s，是下批最明显等待/重复准备段。read-only start 命中 2098/3002 tick，join wall 1.779s 与其中 hydration 1.777s 基本一致；未命中约 904 tick 回退旧路径。CPU 密集仍在 continuous shadow initialize 1.825s、borrowed hydration projection 1.777s、continuous input prepare 1.520s、seal 1.227s。18 份源码已原字节恢复并删除临时 module；首次插桩 build 曾因 `engine::` 路径写错失败，修正后 exit0。详见 `current-critical-path-findings.md`。1000x 未达成，frontend 继续暂停。

## 追加：NPC queue 嵌套诊断（2026-10-06）

针对 `npc_queue_next_tick` 再做 5-span 临时插桩。诊断吞吐 161.059x，样本前半段约 5 秒低 CPU 窗口，疑似外部扰动，因此只用于相对定位。结果：`capture_decision_snapshot` wall 2.745s / CPU 0.852s；`stream_npc_decisions` 外层 0.425s / 0.349s；31,011 个 projection callback 0.196s / 0.165s；31,011 个策略 decide 0.262s / 0.210s；11,840 个 `receive_private_intent` 仅 0.034s / 0.023s。结论：串行 receive 不是瓶颈，下一步拆 `capture_decision_snapshot` 内部。首次诊断 build 因生成 FFI 单行语法错误失败，修正后 exit0；4 份源码已原字节恢复、临时 module 删除。详见 `npc-queue-findings.md`。1000x 未达成，frontend 继续暂停。

## 追加：NPC snapshot 内部诊断（2026-10-06）

对 `capture_decision_snapshot` 再做 10-span 插桩，诊断 220.467x，与正常区间接近。主要成本为：`retail_analysis` 0.865s wall / 0.454s CPU；62,022 次 `CapturedExperienceObservation::capture` 合计 0.844s wall / 0.772s CPU。build_market_view、attention mutate、working order capture、behavior market、retail experience install、DecisionSnapshot::new 每项仅约 0.03–0.17s。结论：下一步优先共享/复用 snapshot capture 内的 price paths 与 behavior market（retail_analysis 内部重复捕获 DecisionChainObservation），并优化 experience capture 的 positions/BTreeSet/clone 工作；不是继续调 Rayon 调度。插桩期间有三次诊断脚本语法/scope 错误，最终 build exit0 后采样，源码原字节恢复、git diff 干净。详见 `npc-snapshot-findings.md`。1000x 未达成，frontend 继续暂停。

2026-10-06 shared price paths 实验已撤回：行为等价测试通过，但相邻对照 candidate 220.049x vs baseline 221.646x，无收益（约-0.72%）；源码已恢复 HEAD，证据见 shared-price-paths-findings.md。两个既有 NPC 测试 fixture 因覆盖策略丢失 base_observation_probability 在 68db9414 基线同样失败，与性能实验无关。

2026-10-06 experience capture 6-span 诊断完成并撤回：clone/get/positions 等六段合计约0.654s wall、0.469s CPU，最大单段 clone 仅0.144s wall；该路径不是1000x主杠杆。下一步转 continuous_shadow_initialize/borrowed_existing_projection/continuous_input_prepare/seal_total 与 due lifecycle O(n×到期数) 扫描。详见 experience-capture-findings.md。之后独立复核恢复使用 collaboration__spawn_agent 新建一次性 reviewer，不再用 codex_tui。

2026-10-06 due lifecycle 索引化方向已量化否决：零侵入临时 example 诊断（已按 HEAD 原字节回滚）显示 3000 帧中约 904 个 read-only start “未命中”实为 900 个 PreOpen/CallAuction 阶段帧（设计上不可命中）+ 仅 4 个真实到期 miss（Continuous 命中 2096/2100=99.8%，空 ledger 0）；lifecycles 峰值 3428、Continuous 期 Σlen≈460 万次比较≈数毫秒（<0.1%），索引化预期收益远低于 2% 门槛，不做实现实验、无源码改动。注意 cargo test --release --lib 因 test profile 默认 debug-assertions 使引擎热路径放慢 >100 倍（299s 超时两次），性能诊断一律走 release example。详见 due-lifecycle-index-findings.md。下一优先级维持四大 CPU 段（shadow_initialize/borrowed_projection/input_prepare/seal）。

2026-10-06 E2 融合批次：read-only 准备第三分支在 capture 后立即 continuous_shards 初始化逐股 coordinator（from_post_expiry 只消费已脱离 input、不读 session，提前执行安全），消除与 StockProcessing 相位之间的屏障；输入捕获错误与初始化错误仍在原首错位置传播，miss/PreOpen/auction 路径不变。先做 21 sub-span 嵌套诊断（已按 manifest 原字节撤回）：shadow_step_zero_ops 跨 worker 2598ms CPU、seal_unknown_checks 885ms、borrowed_existing_projection 内 sort_identity 533ms 为新量化热点；accounts.clone() 仅 8ms 非问题。TDD 真实红灯（FROM_POST_EXPIRY_CALLS 计数器，首轮复核发现并行 harness 污染后改为增量断言）+ 全 tick 等价测试。终版正式基准（1548a88d27ecfc86）：225.665x/228.012x 对相邻 HEAD 221.822x/216.893x（均值 +3.41%，全部 candidate 样本与全部 HEAD 样本区间分离）；修复前源码另测 230.29x/230.12x。三轮独立 agent（实现外）复核：首轮 1 阻塞已修复，终审 PASS。定向短测全绿（read_only_start 7/ctt 19/matching 69/quote_expiry 28/ledger 94 等，9s TERM、RAYON4、四进程并行）。详见 fused-shard-init-findings.md。1000x 未达成，CPU 约 6.0 核；下一优先级：hydration 排序/校验、seal_unknown_checks 账户查找融合、双键重复排序。

2026-10-06 E3 capture 侧双排序融合已提交：两轮可撤回插桩（5-span 分支 wall + per-item 分布，均原字节撤回）纠正了 E2 的长杆判断——post-E2 布局下 read-only join 真长杆是 branch2b（capture 663µs + shards 791µs ≈1454µs/tick），hydration 仅 768µs；大股在两段都占关键路径，逐股流水无解。落地未否决的③：prepare_stock_input 用一次 (AccountId, OrderId, u8) Copy 元组排序同时承担查重（相邻相等标簿序较大者，与旧 BTreeSet 标记集合一致）与 identity 序发射（单股内 ≡ 完整 EnvelopeKey 序），删除 BTreeSet 增量插入与第二次 String 全键排序。红灯（计数器 delta=1024）→绿，15 组定向短测全绿。相邻配对基准 candidate 232.730/234.773 vs HEAD 223.273/224.746（均值 +4.35%，区间分离），verify-build 通过，HEAD 基线与 E2 终版冻结 1548a88d 逐字一致。独立二级 agent 复核 PASS 无阻塞；计数器测试注释按意见改写（仅测试文件，release 30ce134d 与基准产物逐字一致）；重复身份防御路径 fixture 缺口为重构前既有（place/restore 均拒绝重复 id、BookState pub(super) 无注入路径），如实登记。详见 fused-identity-sort-findings.md。1000x 未达成，frontend 继续暂停。

2026-10-06 E4 零 op 初始化去重复校验实验已撤回：8 sub-span 诊断确认 step_zero_ops 2598ms 是"零 op 初始化"四遍全量扫描（validate_initial_snapshots 1297ms、complete_evidence 536ms 含逐行重跑 Envelope::validate、private_market_validate 525ms、ledger_new 192ms），非调度开销。两处可证等价改动（跳过死代码重验证 + Vec<&Order> 引用替代全簿克隆）TDD 红绿、16 组短测绿，但相邻配对 candidate 244.087x vs HEAD 241.261x 仅 +1.17%，低于 2% 门槛，按 HEAD 原字节撤回并重建绑定（probe SHA 回到 30ce134d）。教训：branch2b wall 长杆不随验证 CPU 线性下降，"删可证冗余校验"类刀口收益已到上限；下一杠杆转向 join 外串行相位（candidate_commit/npc_queue/seal_unknown_checks）或行物化成本本身。另登记第三个既有基线失败：shared_ingress 的 failed_private_receipt_gap 与 discarded_candidate 两用例（HEAD 原字节同样失败，与本实验无关）。详见 e4-dedup-init-findings.md。

2026-10-06 E5 捕获通道合并已撤回：诊断确认 candidate_commit 的 1.08s"等待"即内嵌 npc_queue→capture_decision_snapshot 内两个串行 par 相位（experience 捕获 0.844s wall + retail 分析 0.865s wall，状态域不相交）；合并为单通道后 9 对相邻配对基准均值仅 +1.66%（区间重叠、后两对翻负），低于 2% 门槛，源码已按 HEAD 原字节恢复。机制结论：每 tick 到期账户仅 ~20 个、retail 单条更肥，少条目延迟受限下相位合并只省屏障不省轮次（上限 ~1-1.5%）；wall−CPU 差是尾延迟而非可调度空闲，后续应削单条 item 工作量而非调调度。E2 式重叠测试需双向有界闩锁（rayon join steal 反转导致单向闩锁 50% 抖动）。详见 e5-merged-channel-findings.md。
