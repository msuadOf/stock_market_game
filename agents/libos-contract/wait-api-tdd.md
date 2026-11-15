# LibOS 等待接口测试先行

本轮只写测试。定为 **可接收性判定**：`recv_deadline` 的时限限制空队列的等待，不是按 Sender 的墙钟给消息加时间戳。初次收件包括零时限；到判定点时能取到的数据照常交付，断连照常报告，否则报告超时。判定后发布不应被旧调用吞掉。宿主已进入 abort 时显式报告 `HostAborted`，不以缓冲消息或超时冒充正常退出。不承诺 CPU 何时获调度，也不比较 Native/WASM 的绝对等待数值。

- `packages/libos/tests/wait_api_contract.rs` 与 `packages/libos/examples/libos_wait_api_contract.rs` 调用同一个 `tests/support/wait_api.rs`，覆盖 `Instant` 正/逆序差值、checked 加减和时限为零时的 FIFO/断连/超时，以及普通 worker 与自旋 worker 的缓冲顺序、硬停和断连。另检查 `ShutdownWake` 两种负载的哨兵只发送一次、同行断连不妨碍剩余收件、不会重排已入队消息或泄漏内部 Sender。
- `packages/engine/tests/wasm_wait_primitives.rs` 用 fake 时钟调度，在第一个轮询见空之后，以确定性事件将消息发布到最后一个等待片内，唤醒可以在截止时或之后；另测判定后数据仍可收和宿主 abort 优先。原「准点才看到准备好的消息却判超时」断言替换为相反的目标契约；空队列超时和超限诊断仍严格校验。
- Browser 探针在隔离 DedicatedWorker 中通过 verification-only observer 调用与生产 `wait::recv_deadline` 共用的 adapter；真实时钟 import/等待片 observer 驱动注入，不依赖取时次数。Rust 回调负责发布消息/断连。细/粗两档均测判定点可读、延迟醒来、判定后发布、断连、回退到调用起点之上/之下（都仍高于共享原点）。回退应返回 `ClockRegressed`，不是把 panic 当类型化结果；原点自身已损坏的非法 import 则不由这些用例规定恢复策略。此探针不是并发发布的端到端测量，调度时延由独立性能探针覆盖。

未来运行方式（本轮未编译/运行）：

```sh
ROOT="$PWD"
node scripts/run-long-validation.mjs 10000 -- cargo test --locked -p libos --test wait_api_contract --jobs 3
(cd apps/web-wasm && node "$ROOT/scripts/run-long-validation.mjs" 300000 -- \
  cargo +nightly-2026-09-05 rustc --locked --release -p libos --example libos_wait_api_contract --features verification-harness \
  --target wasm32-unknown-unknown --target-dir "$TARGET" --jobs 3 -- \
  -C link-arg=--initial-memory=16777216)
node scripts/run-long-validation.mjs 300000 -- node scripts/fixtures/libos-wait-api-browser.mjs \
  "$TARGET/wasm32-unknown-unknown/release/examples/libos_wait_api_contract.wasm" "$CHROMIUM"
```

补充：`packages/libos/tests/construction_cleanup_contract.rs` 单测覆盖无登记与部分登记的构造失败、未启动与未移交所有权、真线程停止标记先于唤醒、清理诊断不得覆盖 Native 原 panic。三个不依赖物理线程的 API 用例由 Browser 探针运行同一份代码。Browser `initialization` 用独立 Memory 直接调用真实 `init_wasm_wait`，覆盖未初始化时收件和布局都拒绝、两种非法轮询配置不污染状态、合法配置可重试、重复调用不改变 spin 配置和两个状态字地址、不吞已缓冲消息。仍只写测试，未编译、未执行。
