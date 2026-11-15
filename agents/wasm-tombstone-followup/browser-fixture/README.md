# release WASM 浏览器验证 fixture

本 fixture 使用 `verification-harness` 专用的真实 spawn、join 和 panic 入口，不进入正式导出。需要本地 Vite、Chromium、固定 nightly 工具链及受控 sysroot；浏览器须支持共享 memory 和 COOP/COEP。

先设置一个不存在的 `WASM_PANIC_SYSROOT` 绝对路径，再创建受控 sysroot：

```sh
node --input-type=module -e 'import { preparePinnedSysroot } from "./scripts/wasm-panic/prepare-sysroot.mjs"; await preparePinnedSysroot(process.argv[1], process.argv[2]);' "$(rustup which --toolchain nightly-2026-09-05 rustc)" "$WASM_PANIC_SYSROOT"
```

另须显式设置独立的 `CARGO_TARGET_DIR`、`WASM_PKG` 和 `CARGO_BUILD_JOBS`。从仓库根目录使用进程外 supervisor 以 300000ms 上限运行 `batch.sh`，并为终止和清理保留时间。批次比较受测源码的运行前后哈希，源码变化时必须重新构建与验证。

child 场景检查 death futex、join 死亡信号、记录和 hard trap 缺席；primary 场景检查 engine、ingress 记录和身份。owner 场景检查真实浏览器 Worker 的错误经 owner 进入故障处理与终止。另有 `full-host.mjs` 使用真实页面、`createWorkerHost`、`createEngineControlOwner` 与独立 verification-harness 注入 engine panic，检查页面错误码、owner 诊断、Worker 终止和 unknown 释放。其 control worker 工厂为可观测 fixture，非正式产品包同链注入；也不代替全仓验收。

`world-stop` 用独立内存分别执行真实 World 的部分 Start、sever 后 Drop、直接 Drop、prepared 回收和 BindCalendar 失败。Rust 验证 publication 没有推进或构造主错与次生错完整；浏览器另等 supervisor 核对四个真实线程的退出，验证没有待回收 slot。 `watchdog` 跑三个真实收件入口，验证 FIFO 和排空后断连。两组都是独立验证制品的证据，不代表已覆盖生产包内的每个出错相位。

默认验证批与单独 `--deferred-contract` 都要求真实 WASM 专用消费者正常后台入队：固定栏杆期间返回且未析构，关闭时实际排空并消费 join，最后通过同一槽世代的 childExit 与 supervisor 零资源证据。显式 `--deferred-observation` 仅用于历史包诊断，准确识别旧 Inline 或运行严格合同；它不是默认验收。

## Reclaimer 的完整生命周期合同（先写测试）

`deferred-lifecycle-contract.test.mjs` 是证据验收器单测；它验证不合格证据会被拒绝，**不是**浏览器实测。真实端点使用同一个页面和 supervisor，各模式独立 Memory：

```sh
WASM_PKG=/absolute/verification-pkg node scripts/run-long-validation.mjs 300000 -- \
  node agents/wasm-tombstone-followup/browser-fixture/run.mjs --deferred-lifecycle-contract
```

`verification-harness` 专用导出 `verification_deferred_lifecycle_contract(stage: &str) -> Result<String, JsValue>` 直接调用产品 owner 和实线队列。默认验证批也执行以下四条独立场景；`--deferred-lifecycle-contract` 只运行这些场景。旧版观测的 Inline 和测试夹具缺失均不能通过新的严格证据判定。

- `saturation`：通过实际消费者中的栏杆冻结队列，从产品的实际容量读出 N；固定编号 0…N−1 必须全部 `queued`；第 N 个值必须由调用线程 `full-inline` 恰好释放一次，不能污染后端诊断。用实际析构对象记录线程来源和顺序，关闭后验证 N 个排队值均由真实消费者 FIFO 恰好释放一次。4096 上限只界定短夹具的规模，不额外对生产规定容量。
- `close`：未启动、重复启动、重复关闭由实际 API 返回可区分的错误或状态，不能创建第二消费者。记录 reject 返回原值（对象身份及实际析构计数），在拒绝返回时该值尚未释放，随后由调用者释放一次；禁止假报成功、自动重启或同步补做。在真正的 close 关闭提交端、进入 drain 等待后，用仅测试可用的栏杆释放消费者。不从 sleep 时长推断 close 阻塞，必须记录关闭进入、栏杆释放、消费者完成、关闭返回。
- `arm-consumer-panic`：实际启动 Reclaimer，提交真正会 `panic!("LIBOS_RECLAIMER_PANIC_PROBE")` 的析构工作，等到真实消费者进栏杆后返回观测；`release-consumer-panic` 仅释放该栏杆，不主动从 engine 抛同名错误。由实际 supervisor 记录同 worker/slot/gen 的 panic 和 incomplete，不能伪造正常退出。
- `spawn-failure`：由页面在 supervisor 的实际 Worker 工厂抛受控构造失败，再检查 `start` 的失败分类、后续提交拒绝和关闭未启动；无残留 Worker、manifest 或待回收槽。这与先成立的消费者 panic 分别验收，不能互相代替。

正常场景的 `drained-and-joined` 只证明 Rust 的排空及 join 结果；页面另外等待同槽世代的 childExit 和 supervisor 零资源证据。入口只用于测试，不改发布包和默认观测流程。构建与运行是否通过以实际验收记录为准。Native 的隔离真队列用例位于 `packages/libos/tests/deferred_reclamation_contract.rs`，须启用 `verification-harness`。

## 真实 Worker 的 join 与宿主回收合同

独立运行以下三个真实 Chromium 场景（默认批次也包含这些场景）：

```sh
WASM_PKG=/absolute/verification-pkg node scripts/run-long-validation.mjs 300000 -- \
  node agents/wasm-tombstone-followup/browser-fixture/run.mjs --join-lifecycle-contract
```

三个场景各用独立 Shared Memory、真实 Engine Worker、真实 Child Worker 及生产 supervisor。测试专用 `verification_worker_handle_spawn` 和 `verification_worker_handle_join` 直接调用公共 `WorkerHandle::spawn/join`，不绕到底层 raw Builder。Child 走生产 bootstrap/真实 Rust trampoline，在真实 `__wbindgen_thread_destroy` 调用前停在独立 SAB 闸门；读取实际槽字的 `DONE`，取得本 case 真实子闭包的 join 值后再读 `CONSUMED`，并要求第二次消费明确拒绝。期间 child 仍由 supervisor 持有、drain 尚未完成。不开闸期间不靠 sleep 或等待超时猜测该状态。

- **正常关闭**：开闸，包装器计数证明 raw destroy 被调用并成功返回恰好一次；生产 supervisor 观察同 Worker / slot / gen 的真实 childExit、零资源 drain，最后只报一次成功 Memory 回执。child 没有被 owner terminate。
- **清理注入失败**：开闸后，由真实 child 在 cleanup 包装器内、实际 raw 调用前抛出专用错误；生产 bootstrap 必须经其 cleanup 失败通道上报原错误。没有 raw destroy 成功或正常回收证明，drain 必须拒绝并报告 unknown。
- **强制终止**：闸门保持关闭，由生产 supervisor 对真实 Child Worker 调用 native terminate；记录实际调用，拒绝 drain 并报告 unknown。这不证明底层已执行 TLS/stack destructor，也不等同于前一条 cleanup 抛错。

通过正常验收要求确认 raw 销毁调用与回执链，不声称直接逐字节验证底层分配器或宿主 RSS。上述新增场景当前只写了代码，尚未编译或运行。
