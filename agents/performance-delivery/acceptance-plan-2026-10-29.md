# 同条件性能验收准备（2026-10-29）

**原准备记录与后续入口交付增量，未构建、未测量。** 顺序固定：engine正确 → #122 → 真实宿主 → 同条件性能 → 正式all → 本地main。初始开发锚 `7dff5856fbc99e8fdc15d9446e052e4c6b608d58`；后继独占入口树基于 `8631320b3ea68de0f9dd4f67df50ca65a6ef5750`，最终待验OID/输入/binary/feature及CPU集仍须冻结，不拿旧产物代替。

## 下一次验收与现成命令

现有 `server_max_speed_probe` 可供**同一开发源码、同一binary/SHA、同档输入**的serial/runtime代表诊断，不能占正式验收窗冒同量配对；主验收由#122 writer与后继测量owner在同源入口控制精确停止后，比较 `EveryTick`/`DayEnd`。前者的 `ENGINE_STEP_RUNTIME=0/1` **不等于**三档。native开关由OnceLock首读缓存，每样本新进程；单进程一局，Tokio2，runtime生产默认单组+每股线程。两臂顺序测量以免争用，serial单线程是对照路径的已知限制；runtime内部多核，记录进程树CPU/runnable/RSS与实际组数，不把配置线程数称满核。

下列是**前置完成后**的实际CLI形状：`perf_tree`=最终开发checkout；`perf_target`=root批准目录；`perf_bin`=本次Cargo artifact executable；`perf_setup`=该树 `agents/engine-max-performance/default-setup.json`；`perf_cpus`=当窗两臂相同的可用CPU集（原目标16物理核，不能假定0–15空闲）。

```sh
# 构建单独占窗，真实exit0后冻结artifact；编译不计执行wall。
CARGO_INCREMENTAL=0 node "$perf_tree/scripts/run-long-validation.mjs" 300000 -- \
  cargo build --manifest-path "$perf_tree/Cargo.toml" --locked --release \
  -p server --example server_max_speed_probe -j8 --target-dir "$perf_target" --message-format=json
# 代表配对：两个全新进程共用一次进程外deadline；正式重复也须共用批deadline。
node "$perf_tree/scripts/run-long-validation.mjs" 300000 -- sh -c '
  for mode in 0 1; do
    taskset -c "$3" env ENGINE_STEP_RUNTIME="$mode" ENGINE_SPIN_US=500 "$1" "$2" 3000 || exit "$?"
  done' perf-pair "$perf_bin" "$perf_setup" "$perf_cpus"
```

负载：**5股、20000 retail+5 inst+2 hot、seed42、请求至少3000tick、日长15300/开900/收180、T+1**。server消费整批frames后才检查下界，终点可超过3000；记录实际ticks/末tick与overshoot，两臂可能终点不同，故此代表比较**不证明精确同tick工作量的提速**。逐帧连续且无failure才有效；按实际 `ticks/wall_s`，初始化单列，计时内lazy构造照计。不含HTTP/WS/Browser。example会合并dev依赖 `verification-harness`，两臂必须同features；不能报普通production达标。旧独立 `production-server-probe` 仍三参数/Rayon注入，与当前二参数example漂移，其verifier会拒；`cpu-probe.py` 也硬传第三参数，暂不能照旧README运行。

## #122 的主配对门

按[既定合同§6/7](../perf-intraday-independent/convergence-contract.md#7-配对测量计划决定默认值前必须完成本轮不设门)：同release binary/SHA，seed/config仅policy不同，**ABBA至少5轮、剔除热身**；覆盖完整交易日+自然日日结+次交易日首拍。固定CPU/组数、profile/features、allocator、工具链/lock、功能开关与事件交付。报告日内t/s、含flush/civil的整日wall、flush P50/P95/max、峰值内存与N/2N增长斜率；覆盖表、PlanBook/保留事实、总RSS分别归因。**无吞吐/RSS通过阈值，不翻默认；1000x实时倍速不等于after/before提升1000倍。**

初次准备时E3字段与测量入口未实现；**此项现由A交付的 `SessionSetup.runtime_sync_policy` 三值snake_case与下述新入口取代**，B完整flush/save/directstep/join仍须统一实际通过。旧 `driver_semantics_probe <setup.json> <ticks> <groups>` 拒serial，现有wall/events/有限守恒不足§7，15300日长的3000tick也不跨日。正确性先过合同#1–16/#18；确定性跨档fixture≤2组/≤2股/≤3交易日，同脚本日终各域逐字段相等；同次World→mirror逐分逐股、事实恰一次、日界/失败原子性一致。自由并发不要求跨运行成交顺序、事件数或整局字节相同；不能比较缺功能路径。

## 原1000x补充锚（不替代上述配对）

| 口径 | 精确baseline与条件/可复用命令 |
|---|---|
| 原目标起点 | `a600149469a7d14be5b3dc682119ce07012af991`；[原findings](../engine-max-performance/findings.md)：5股/20007 NPC/seed42/16核，一市场tick=一秒；旧Rayon16的3000tick=78.202 tick/s，不能作当前std-thread同功能分母。 |
| B全语义 | commit **`4315dbd5eb32b05d298c11fb668dd8472a721687`**；annotated tag对象`0ae57763129c2f93a7bece3bc7e64bc6b5e745b7`。历史1032.7 tick/s；两侧新构建后同窗各跑 `taskset -c 0-31 env ENGINE_SPIN_US=500 ENGINE_STEP_RUNTIME=1 <B_BIN> <同字节setup> 3000 1`。现`.tmp/driver-semantics-setup.json`为5股/20000纯retail、15300/900/180，SHA256 `da1cb3dffc1583eb01d7994ea224b782edbcc9b373ae93c7a2baba5d83c3ade7`；原b-profile头写9股，**历史输入字节关联待核**，不能用现文件冒旧封存输入。 |
| A裸World | commit **`d07726af6d6c1745858e3c38391f5fbcc718abdc`**；annotated tag对象`5e0f7269b15a6fd54709803974ca430a709dad4a`。历史1011.8 tick/s；`taskset -c 0-31 env ENGINE_SPIN_US=500 <A_BIN> 3000 10 2000`；5股/20000 retail、日长240/开0/收3、probe固定seed。无Session安装，不能与B/server相除；日界失败原件保留。 |

A/B在各自baseline与最终开发checkout分别预构建：`cargo build --locked --release -p engine --example driver_semantics_probe --example runtime_s5_probe -j8 --target-dir <获准目录> --message-format=json`，外包既有300000ms supervisor/INC0；实际源码/输入/features与binary绑定后才测。B按实测wall计冷启动，不扣历史≈330ms，不拿1000tick短窗直比3000tick。

**资源与交付：** 一次一个完整批；构建j8/INC0与执行分窗。我方构建/benchmark不重叠；其它用户/项目qemu、spike等负载不能擅停，记录load、CPU亲和和同核干扰。§7正式口径要求整机独占；无法取得时先在可用核做同条件代表配对，明确“共享负载、受干扰”，不冒称独占正式速度，也不无限等待。全部child/校验/发布/清场共享进程外300000ms，前299000ms执行/校验/小报告，末1000ms只终止清理。预算不足记失败/未完成，不重置每轮deadline、不裁掉跨日、不减NPC冒原目标；普通正确性case/命令10秒、多核隔离。只复用 `run-long-validation.mjs` 与小原件，不新建体系或大包；旧escrow准备链显式退役，通用harness的JSON也不匹配server/B。交原小输出、条件/源码/binary绑定、统计与限制，经未实施者复核后由root推进all/main。

## 2026-10-29 已批准的批边界修订

root与未实施Sol已明确批准：独立编译批后，预登记5个独立ABBA长验收批；每批热身E/D各1次再跑E,D,D,E，批内执行/校验/发布≤299000ms，最后1000ms仅终止清理，各自进程外300000ms监督，禁止逐sample重置。5批顺序、同source/binary/setup/CPU，各保load/干扰及所有失败原件；仅5批完整才汇总完整配对数据，不称单个300秒全过。原负载/跨日/至少5轮ABBA/无性能阈值不变。此段明确取代上文“全部配对样本共享单个300秒”的批边界，属于本次批准的新修订，不能冒称原文早已允许。

预算依据：原15300tick完整日+次首拍每样本15301tick。旧单批热身2+5×ABBA4=22次共336622tick，若两臂1000tick/s，纯推进336.622秒已超299秒；这仅解释被替代方案。新每批6次共91806tick，5批共30进程/459030tick，其中10次热身从正式统计剔除，正式20进程/306020tick。各批仍须遵守299秒执行预算，不能据估算承诺可完成，也不裁NPC/日长/轮数。纯自动flush读面经B owner确认不存在，输出null/unavailable；单列最后市场step+真实civil publication/候选捕获的combined boundary wall，不以no-op flush算P95。

实际入口文件是 `packages/engine/examples/runtime_sync_policy_probe.rs` 与 `scripts/performance/runtime-sync-policy.mjs`，runner 的假端点短测在同名 `.test.mjs`；不修改engine核心、Cargo或旧probe。example CLI为 `<setup.json> <every_tick|day_end> <seed> <trading_days>`，用生产 `ProtocolSession::new`（实际默认1组）逐个 `step_frame`，完成指定交易日的 `end_civil_day_update`、真实存档候选捕获，逐休市自然日推进，再到下一交易日首拍后精确停止；原规模1日是15301tick。退出前核实际owned/joined/GroupFinal/DeskFinal，并报告纯flush unavailable及combined boundary wall。runner CLI为 `batch <config.json> <1..5> <新目录>` 与 `aggregate <新报告.json> <五份批报告>`；同一配置预登记5批，逐批外包 `run-long-validation.mjs 300000`，聚合拒绝缺批/重复/失败/输入或binary/CPU漂移。源码交付、假端点单测与Rust实际可编译/真实测量分别报告；integration在真实宿主闭合后统一安排构建和测量。

当前 `COMPLETE_PAIRED_DATA` 只表示五批完整执行数据，`contract_metrics_complete:false` 明列未覆盖的N/2N、域字节及纯flush独立wall，不能等同§7全部性能验收完成。root已同意最窄后继：保持原20007NPC/同setup，预登记一次2N完整自然日诊断（如需策略对照两臂各一次独立长批），在N/2N相同候选生命周期点记录公开SaveSlot域计数、serialized bytes及真实进程树RSS；序列化尺寸不是resident allocation，不扩成N×5ABBA矩阵。N与跨休市范围及每批300秒预算须事前固定；私有覆盖表实载峰目前未观测，结构≤3A复用T13直接证据，不伪造零值或为resident拆解扩core。
