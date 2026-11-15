# Browser 发布 Worker 的故障边界

**当前共享 Memory 架构不能安全隔离硬 Rust panic。** `scripts/wasm-panic/patch-std.mjs` 的受控 std 进入 `wasm_tombstone_early` 后直接 abort，不执行 unwind，也不执行 `MutexGuard` 的析构。`apps/web-wasm/src/market_runner.rs` 的 publisher ABI 会读取或修改共享 `OUTBOUND` registry 及共享 publisher binding 的 `Mutex`，回复的 Rust→JS 序列化还使用同一 WASM 实例组的分配器。hard panic/trap 未证明发生在全部共享资源之外；若把这类故障当作观察者断线，可能遗留锁、结构或不完整的回收状态，甚至在恢复时卡住权威端。因此保留 LibOS 共享 `DEAD` 与整代封锁。若硬 trap 绕过 Rust hook，由宿主报告并安全封锁；没有 tombstone 时不虚构 panic 消息。

**正常投递失败不能关停市场。** UI 拒收、观察者追不上、合法关闭、可判定的传输故障，以及从 Rust 正常返回的可分类错误，不应进入 hard Worker fault 路径。它们由观察层报告并隔离、断开或重建游标；发布任务停止，该观察者的缓冲不得阻断权威提交。没有恢复游标或无法安全重新绑定时，应显式报告观察流不可用，而不是把它冒充成市场故障。不要把 `Result` 返回与 trap 混为一类，也不要把任意来源的异常默认为可恢复。

**验收用例：** 真实 Chromium 中分别注入观察者拒收、可恢复发布端错误和 publisher Rust panic/hard trap。前两类要求通知可见且权威 tick 继续递增；第三类要求共享资源的 generation 被安全封锁，等待者得到死亡通知、没有伪造成功交付或可信回收收据。若将来需要在硬 panic 后仍能安全继续权威推进，须首先消除 publisher 对共享 allocator/registry/锁的依赖或改用真正隔离的地址空间，而不是只移除 `DEAD.store`。
