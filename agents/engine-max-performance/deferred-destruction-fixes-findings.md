# 后台析构四项阻塞修复（2026-10-08）

提交 8cc87c32。依据：71dd7825/71de0ae8 的独立复核四项 FAIL。

## P1：WASM 平台崩溃（正确性）

**问题**：deferred_drop.rs 无条件 `std::thread::Builder::spawn().expect()`，在 wasm32-unknown-unknown（浏览器）上 `std::thread` 恒返回 `Err(UNSUPPORTED_PLATFORM)`——浏览器首次 CommitTick 即 trap。

**修复**：`cfg(target_family = "wasm")` 同步 `drop(value)`，不进 sender/OnceLock 初始化。注意不能按 `target_has_threads` 或 `target_feature = "atomics"` 判断（这些 cfg 在启用 atomics 的 wasm 上也存在但 std Thread 仍 unsupported）。

**验证**：原生路径编译+测试全绿；WASM 目标未安装（本机限制），cfg 结构保证 wasm 路径不含线程/channel 引用（编译期排除）。WASM 路径只保证正确性，不承诺池外性能收益。

## P2：AppendOnlyHistory 末 owner 递归爆栈（并发安全）

**问题**：shared_history.rs Drop 用 `Arc::try_unwrap` 迭代断链。两个跨线程最后 owner 同时失败时各得 Err；最后 Err 的 Arc drop 递归释放 previous 链，长历史（~10k chunks）stack overflow abort。

**修复**：`Arc::try_unwrap` → `Arc::into_inner`（Rust 1.72+ 稳定）。into_inner 原子检查唯一所有权：两线程竞争时恰好一方 Some（迭代断链）、另一方 None（退出）——不会出现双 Err 后的递归释放。

**验证**：新增 `concurrent_last_owner_drop_does_not_recursively_release_chunk_chain` 测试（327680 元素/10240 chunks，Barrier 同步双线程同时 drop）全绿；原有 `releasing_a_long_shared_history_does_not_recurse_through_chunks` 保持绿。

## P3：无界 mpsc 队列（资源安全）

**问题**：deferred_drop.rs 用无界 `mpsc::channel()`，无背压。旧权威状态整份排队，析构线程被饿死时内存可无上界增长。

**修复**：`sync_channel(64)` + `try_send`。满时闭包随 Err 一起 drop，值就地析构（退化路径与线程退出后的兜底相同——语义等价，不阻塞调用方）。64 条上限给出明确积压边界（每批一个 checkpoint + 每 tick 一次 commit_from，正常消费远快于生产）。

**验证**：新增 `many_rapid_defers_do_not_block_or_panic` 测试（超过容量 4 倍的快速连续 defer 不死锁不 panic）全绿。

## P4：全局计数假通过（测试可信度）

**问题 A**：deferred_drop.rs 先计数后 send——发送失败仍增数。**问题 B**：server fatal_tests 的批级 checkpoint 断言用全进程 AtomicU64 前后差≥1，并行测试中其他 defer 也贡献计数。

**修复**：计数移到 `try_send` 返回 Ok 后才递增——退化内联路径不产生假计数（消除问题 A）。问题 B 的结构性解法需要 per-channel 计数或测试串行化，本批仅加注释说明限制；server 测试 35/0 全绿含该测试。

## 测试对照

- engine lib：**1323 过/8 败**（基线 1321/8 + 新增 2 测试全绿；失败名单与本次改动无关）
- server lib：**35 过/0 败**
- probe 构建验证通过（SHA 9fba1329ff6a…）

## 遗留

- WASM 编译验证需安装 wasm32-unknown-unknown target 后补跑
- P4 问题 B（并行测试共享计数）需结构性修复（per-channel 计数或专用测试隔离），当前注释说明
