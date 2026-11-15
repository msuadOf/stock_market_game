# LibOS API 性能对照（先写测试）

没有编译、运行或采集本批任何基准，也没有修改生产实现。原有热态与析构场景仍保留。

| 探针 | 测量窗 | 对照与可用证据 |
| --- | --- | --- |
| `clock-now` / `buffered-recv` | monotonic 取时；预填队列的协调者热收件 | Native / Browser，同制品、同底座；不含等待与资源销毁 |
| `idle-deadline` | 保持连接的空通道多次实际等待 100 μs | 两臂同样必须报 Deadline，并在超时判定点重新检查队列。Native 对 std `recv_timeout` 加超时后的 `try_recv`；Browser 两臂读同一个真实 abort 字、用相同单调时钟和轮询预算、调用同一个 std sleep。该探针不替代带边界消息的语义测试。 |
| `spawn-join` | 新线程创建、执行固定值闭包并完成 join | Native 对同样具名的 std thread；不得用闭包结束代替 join。 |
| `message-handoff` | 在途始终一条消息且逐条核对回执，排除 spawn/join | Native 对同一 mpsc/原子策略。是双向交接，不声称线程实际 park 或测得单向唤醒。 |
| `shutdown-wake` | 已登记的两个持久连接上发布硬停止，直至收到两个停止回执 | Native 对同一原子顺序、同样的唤醒哨兵和发送端拆除。注册、spawn、join 在计时外。接收入口可能尚未 park；不以 kill 操作缩短测量窗。 |
| `queued-drop-return` / `queued-drop-complete` | 后台提交返回与实际 FIFO 析构分开 | Native 使用同一真实有界队列，两臂都要求成功排队并核对析构完成。 |

用法沿用 [边界探针的构建和运行命令](../../scripts/performance/README.md#libos-边界局部成本不同于产品前后性能比较)。普通 Node 验证预期命令：

```sh
node scripts/run-long-validation.mjs 10000 -- node --test --test-timeout=10000 scripts/performance/libos-boundary-cost.test.mjs
```

配对使用同制品、隔离 CPU 上交错的 31 对样本，要求报告计时量化裕量和完整原始数据。p95 是**批均值**的尾部，不是单次操作 p95。欲判定是否退化，在正式采样前提供 `--budget <绝对 JSON 路径>`：`schema` 为 `libos-boundary-budget-v1`、`target` 为 `native` 或 `browser`、`cases` 为每一项可判定的操作对应的 `maxMedianDeltaNs` 与 `maxMedianDeltaFraction`。不得为 `characterizationOnly` 项提供通过预算。阈值应由使用方核定，不从待验收样本倒推。完整量化调整区间在两个阈值以内才 pass；跨线为 inconclusive，整条区间超出阈值才是 regression，后两者都以非零状态结束但保留报告。未提供预算时只生成观察值。两平台不能用同一计时绝对值代替各自的零封装成本论证。预算只证明被量的场景，不是整个 LibOS 的通过标记。

## Browser 真实 Worker 对照

`browser-perf/run.mjs` 使用开启 `verification-harness` 的独立 Release `web-wasm` 制品、应用的 shared Memory / EngineSupervisor / child bootstrap，以及不携带验证导出的正常产品构建之外的两条路径。两臂不拿普通 JS `new Worker` 与 Rust/slot/监督协议相减：

- `spawn-join`：同一 WASM `Builder + JoinHandle` 与 `WorkerHandle`，用相同任务与 runtime name。分别记录 Rust 开始到取得结果的时间，以及页面从请求开始到 W3 / slot 回收；后一项两臂都包括相同宿主消息链。
- `message-handoff`：两端共用 Browser Worker、mpsc、时钟与宿主 abort 字；直连路径使用匹配的 stop/FIFO/断连规则和零自旋/零安静预算，门面调用 `worker_recv`。只测真实回执的往返，不将测得值当作单向延迟或已 park 的证明。
- `shutdown-wake`：保持两个通道各有额外 sender，真实两个 Worker，分别比较直连原子+哨兵与 `ShutdownWake::sever`，佐证哨兵不作业务值。以双停止回执为计时终点，真实退出另验。
- `deferred`：直连独立 Worker 的有界队列与实际 LibOS Reclaimer 同为排队；用独立 FIFO 标记证明被测析构都已返回，分别记录提交返回及全部工作完成。两边另等实际关闭、join 与 Worker 退出。WASM owner 的真实 `start_wasm_reclaimer`、`close_wasm_reclaimer` 或 `Queued` 语义缺失时显式失败，不产出成功证据。

每臂均验 Worker、slot、generation 身份和零残留，不以 engine 报的 `DONE` 代替宿主退出。用实际 JS 页面与 Rust Engine Worker 的较粗实测计时量化值判定采样有效性；异步回收每臂用新的 Memory（初始化时间不计入 Rust 窗口）。裸性能探针不再测历史 inline 析构。

```sh
# 先按 browser-fixture/batch.sh 的同一受控 sysroot / verification-harness 构建说明准备
# WASM_PKG；这里只消费构建完成的制品，CARGO_BUILD_JOBS 必须在构建时显式设置。
WASM_PKG="$ABS_WASM_PKG" node scripts/run-long-validation.mjs 300000 -- taskset -c "$CPU_SET" \
  node agents/libos-contract/browser-perf/run.mjs spawn-join "$ABS_NEW_REPORT" "$ABS_CHROMIUM" "$ABS_BUDGET"
node scripts/run-long-validation.mjs 10000 -- node --test --test-timeout=10000 \
  agents/libos-contract/browser-perf/report.test.mjs
```

报告预算可省略（只记录数据），预算需按单个 probe 输出的指标命名，不能套用跨平台绝对耗时。满队列、竞争压测和资源峰值（CPU/RSS、分配与队列高水位）不是该组同底座操作延迟证据，应与已有产品性能报告单独核对。
