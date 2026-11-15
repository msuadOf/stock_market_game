# 行情 UI 最快档性能回归

脚本用本机 Chrome/Edge 的 DevTools Protocol 驱动真实页面，不引入项目级 E2E 框架依赖。它会：

1. 复用 `--url` 指定的页面；页面未启动时自动启动 Vite。
2. 使用 390×1180 移动端视口，通过现有启动选择明确选择本地并启动游戏，进入第一只股票详情并切到“最快”。启动错误显式终止，不改变产品的启动确认政策。
3. 分别观察分时图与日 K，验证权威游戏 tick 和图表签名都在推进。
4. 采集 Chrome 的任务、脚本、布局、堆内存、DOM 节点、长任务、布局偏移和近似 FPS。
5. 生成 `report.html`、`report.json` 和四张前后对比截图。

运行：

```bash
corepack pnpm test:market-performance
```

可选参数：

```bash
corepack pnpm test:market-performance -- --duration-ms 10000 --url http://127.0.0.1:5173/ --output artifacts/my-run
```

报告默认位于 `artifacts/market-ui-performance/<时间戳>/`。任何一个推进检查失败，进程都会返回非零退出码，并在初始化失败时写入 `failure.txt`。

此命令是长验收而非普通单测，由进程外 `run-long-validation.mjs 300000` 监督完整批次；最后 1000ms 预留终止进程树。每项资源清理失败仍继续清理其余资源并汇总报错。真实浏览器旅程仍需本机浏览器、WASM 构建及 COOP/COEP 环境，不以短 fixture 测试代替性能报告。

## LibOS 同平台性能配对

`libos-paired-performance.mjs` 比较**已采集**的两个 JSON 文件，不启动产品。配套的 `libos-collect-performance.mjs` 运行产品并生成这些文件。Native Server API/WS 和浏览器真实 shared-memory WASM Worker 分开运行、分开判定；不比较两者的绝对速度。示例：

```bash
node scripts/run-long-validation.mjs 300000 -- node scripts/performance/libos-paired-performance.mjs \
  baseline.json candidate.json thresholds.json > comparison.json
```

采集时固定机器、系统／浏览器版本、探针版本、toolchain、release profile、features、CPU 亲和集、实际 worker 数、工作负载源码及测量窗。使用同一硬件隔离采集，不把 baseline 和 candidate 同时跑在同一组 CPU 上；交替先后顺序以降低热状态和负荷漂移。每次运行的宿主仍使用配置所记录的多个 CPU/worker。日志须提供进程树利用率与 Group/Desk 确有工作证据；不可用示例程序、旧串行、Vite 驱动耗时或者 UI FPS 冒充产品引擎指标。Native 记录包含 worker 的 Server 进程 CPU 和 RSS；Browser 单独记录运行该会话的浏览器进程组 CPU 和同一引擎实例经真实释放回执读到的线性内存页数，**不得用 JS 堆内存代替**。本轮历史配对所用正式默认拓扑只有一个 Group，如实登记，不作为二组拓扑的证据。

每个测量文件的 `schema` 为 `libos-performance-receipt-v1`，顶层还需 `sourceSha`（40 位 Git commit）、`sourceTreeSha256`、`lockSha256`、`artifactSha256`、`context` 和 `samples`。源码树摘要必须覆盖未提交差异（若有），产物摘要须绑定实际运行的 Native 可执行制品或浏览器发布产物清单。判定器校验标识格式、配对条件及各样本事实一致性，但不会重算外部制品摘要，也不把 JSON 声明当作运行证明；须另保存原始探针输出与制品记录。

- `context` 的精确字段：`target`（`native` 或 `browser`）、`platform`、`machineId`、`host`（分别为 `native-server-api-ws`、`browser-worker-shared-wasm`）、`hostVersion`、`executionVersion`、`toolchain`、`profile`（`release`）、`features`（排序）、`cpuSet`（升序编号）、`workerCount`、`workloadSha256`（含固定 seed、输入轨迹和受理观测口径）、`warmupTicks`、`measuredTicks`（至少 20）。这组字段必须完全一致。
- `samples` 需至少三对奇数次独立采样：`pairId` 匹配，`measurementId` 不可复用。使用纳秒整数记录 `elapsedNs`（含实际测量窗内的步进与日结，不含初始化）、`cpuNs`（相同测量窗、全部参与进程）、`startupNs`（进程或 worker 会话启动到首次可接收操作）、`shutdownNs`（发出结束命令到会话完成回收）、`tickNs`（已提交 tick 的原始顺序耗时列表）。`memoryPeak` 为 Native 进程 RSS 字节或 Browser 共享 WASM 页，Native 以 10ms 间距采样，并覆盖测量窗两端（采样最高值，不声称捕获所有瞬时 RSS 峰）；Browser 使用同一只单调增长的共享 Memory 的完整会话高水位（包括初始化、恢复及收尾），不是独立稳态窗口的页数。
- 每个样本的 `authority` 必填 `firstCommittedTick`、`lastCommittedTick`、`lastCommittedSeq`、`readyReceiptSha256`、`factsSha256`、`activeGroups`、`deskExchanges`（`["Shanghai","Shenzhen"]`）、`playerAccepts`、`npcOrders`。采集器的受理证据来自实测入队确认后的输入、已发布结果和日终存档中的 ingress receipt 游标，不能替代逐条内部 Ready receipt。两种摘要保留实测受理／提交顺序；先按现有协议规整传输格式，不能重排有业务含义的数据。产品或探针没有相应观测量、采样窗口改变、没有真 NPC 或玩家活动时，保留原始证据并记为不可比较。
- `thresholds.json` 必须在采集前明确 `throughputLossPct`、`p50RisePct`、`p95RisePct`、`cpuPerTickRisePct`、`memoryRisePct`、`startupRisePct`、`shutdownRisePct`，没有默认门槛。报告包含每对指标及按最近秩计算的 p50 / p95，每个门槛按同平台各对相对变化的中位数判定；判定器本身的退出码 `0` 为通过、`1` 为越线、`2` 为不可比较或参数错误；上面的外部监督器统一用非零退出表示失败。越线会输出结构化比较结果，不可比较会向 stderr 写具体原因。

### 采集历史 baseline 与当前改动

先在相同机器上准备干净的历史源码 `4961bd2740d4ffbed83528727cb571047c44bb3d` 与 candidate 独立 checkout。两边都安装锁定的 Node 依赖（`corepack pnpm install --frozen-lockfile`）。Native 在每边运行相同的 `cargo build --locked --release -p server --bin server --features host-parity --jobs "$CARGO_BUILD_JOBS"`，各自保存产物和构建日志，不把一侧的可执行文件复制给另一侧。Browser 在两边保留各自正式 release shared-memory WASM 包及已生成的 provenance 文件，不运行 verification-harness。浏览器可执行文件、工具链、CPU 亲和集保持不变。阈值需先写好并保存在源码目录之外；输出也放在源码目录之外。

```bash
node scripts/run-long-validation.mjs 300000 -- taskset -c "$CPU_SET" node scripts/performance/libos-collect-performance.mjs \
  native "$BASELINE_ROOT" "$CANDIDATE_ROOT" "$BASELINE_SERVER" "$CANDIDATE_SERVER" "$THRESHOLDS_JSON" "$NEW_REPORT_DIR"
node scripts/run-long-validation.mjs 300000 -- taskset -c "$CPU_SET" node scripts/performance/libos-collect-performance.mjs \
  browser "$BASELINE_ROOT" "$CANDIDATE_ROOT" "$CHROMIUM" "$CHROMIUM" "$THRESHOLDS_JSON" "$ANOTHER_NEW_REPORT_DIR"
```

每个平台交错采集 3 对。计时包含受控产品单步的真实宿主调用及公开事实到达（不是 Rust 内部独占耗时）；从固定预热后持续到首个自然日结算，存档/恢复验证在计时窗外。Native 的 CPU 为 Server 进程内全部线程的活动时间，不含探针；启动截至真正的首个会话快照，收尾截至删除会话成功。Browser CPU 由 CDP 采集该独立 Chromium 实例的进程 CPU，观测集合变化则拒绝配对；启动按 Worker Host 到首个会话快照，收尾按 dispose 完成。Native 记录实际可执行文件摘要，但工具无法仅由二进制反证具体编译参数，需配合锁定构建日志核验制品来源。Browser 显式核对各自 release 制品的源码身份和内容摘要。测量失败时保留已取得的逐次原始采样，不产生通过结论。

既有 `production_entry_performance`、`production_actor_performance` 与浏览器 UI 探针仅供对照各层耗时与资源口径，不能代替同一产品入口配对。

## LibOS 边界局部成本（不同于产品前后性能比较）

`libos_boundary_cost` 是单独的 release example，不会链接到产品。两臂在**同一份**可执行制品/同一个真实 Browser Worker 内运行，使用相同操作、编译优化、常量、硬件与计时源：

- `clock-now`：Native 用 `std::time::Instant::now` 对 LibOS 同名 re-export；两侧在热循环执行相同的单调性断言。在 WASM 没有可用的 std 时钟，因此直连臂调用相同 `engine_clock_now` import 并执行等价的原点原子读取、纳秒转换与倒退/溢出检查。两臂分别初始化原点，不测试多 Worker 锚定、初始化或异常成本。
- `buffered-recv`：仅测预填、无并发发送/终止、必成功的 coordinator 收件，每次检查总和及空队列。Native 基线用相同的 `recv_timeout(50ms)`，Browser 基线用相同队列的 `try_recv` 与两次确认同一个 LibOS host abort 字，并保留热路径两次 LibOS 取时。队列预填和回收不计入耗时；WASM 直连不重复查找等待配置或带入冷分支，量化的正是对已确认可立即取件的成功路径施加该通用政策后的局部成本。正式计时前在另一实例验证两臂实际时钟调用次数相同。该对照不覆盖冷队列、睡眠、阻塞、断开、DEAD、超时、切换 Worker 或跨线程竞争。
- Native `queued-drop-return` / `queued-drop-complete`：同一个实际 bounded 后台队列、相同 Box/析构值、相同 `try_send`，分别比对 API 返回和带 FIFO marker 的完成时间。每段最多 63 次（容量 64 的实队列给完成 marker 保留一槽）、只接受两臂均排队；每段都以 marker 验证已实际析构，返回计时不含 marker，完成计时包含。报告为这种分段的平均每操作成本，不是单个析构的独立延迟分布。饱和、失败退化、启动、复杂析构和跨进程不计。Native 启用 verification-harness 以读取真实 sender，接收测量走正常 Native 接口。
- Browser 后台析构不在这个裸探针中计时：裸 WASM 实例没有真实 Worker supervisor 与 reclaimer owner，不能借同步析构冒充。用 [真实 Browser Worker 对照](../../agents/libos-contract/performance-tdd.md#browser-真实-worker-对照) 测正常入队返回、实际完成与物理退出。
- 计时前校准同一 batch 操作数，目标至少 2ms / 200 个观察间隔，正式样本拒绝低于 100 个观察间隔。每项 AB/BA 交替 31 对，输出两臂中位数、配对差中位数、名义覆盖率至少 95% 的符号区间和另加两倍观察计时间隔的区间。该区间不是实测平台的一般上界；计时噪声和相邻批次相关性不保证符合独立模型。区间包含 0 时表示本采样未能分辨，不宣称数学上零开销。这里不设隐含合格阈值。
- Browser 需要实际 Chromium DedicatedWorker + COOP/COEP，使用同一实例真实 shared WASM 线性内存。裸 Browser 探针在计时之外验证时钟和 worker/coordinator 收件；使用独立实例，正式计时不装 probe。将 artifact 单独在 Node WebAssembly import/instantiate 做 smoke 不算获得浏览器运行结果。此探针不是产品 WASM 也不注入测试导出到产品。

```bash
ROOT="$PWD"
TARGET="$ROOT/.tmp/libos-boundary-target"
node scripts/run-long-validation.mjs 300000 -- cargo build \\
  --locked --release -p libos --example libos_boundary_cost --features verification-harness \
  --target-dir "$TARGET" --jobs "$CARGO_BUILD_JOBS"
# 从 apps/web-wasm 运行，加载产品固定的 atomics / shared memory / build-std 配置。
(cd apps/web-wasm && node "$ROOT/scripts/run-long-validation.mjs" 300000 -- \\
  cargo +nightly-2026-09-05 rustc --locked --release -p libos --example libos_boundary_cost \\
    --features verification-harness --target wasm32-unknown-unknown \
    --target-dir "$TARGET" --jobs "$CARGO_BUILD_JOBS" -- \\
    -C link-arg=--initial-memory=16777216)
node scripts/run-long-validation.mjs 300000 -- taskset -c "$CPU_SET" node scripts/performance/libos-boundary-cost.mjs \\
  native "$TARGET/release/examples/libos_boundary_cost" "$NEW_NATIVE_JSON"
node scripts/run-long-validation.mjs 300000 -- taskset -c "$CPU_SET" node scripts/performance/libos-boundary-cost.mjs \\
  browser "$TARGET/wasm32-unknown-unknown/release/examples/libos_boundary_cost.wasm" "$NEW_BROWSER_JSON" "$CHROMIUM"
# 仅验证 Browser Worker 契约，不采集计时对。
node scripts/run-long-validation.mjs 300000 -- taskset -c "$CPU_SET" node scripts/performance/libos-boundary-cost.mjs \
  browser-contracts "$TARGET/wasm32-unknown-unknown/release/examples/libos_boundary_cost.wasm" "$NEW_CONTRACT_JSON" "$CHROMIUM"
```

同一台机器上的两组测量不要与构建或另一个测试套件同时执行。保留构建日志和 JSON 中的 artifact SHA、CPU affinity、Worker 身份与每对原始记录；不能用 Native 的数值代填 Browser，或用任一热路径结果推断整个引擎、spawn/join、饱和排队、一般负载析构或 Worker 生命周期的成本。
