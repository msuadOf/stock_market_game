# D1 storage 中间切片证据

- 基线：`refs/heads/main` 的 `9065ac1a72b74e2eba6c17d362df243c381b746f`；独占分支 `fix/runtime-diagnostics-d1`。
- 设计：冻结 v3 SHA256 `22dfed9ad2f9b2e1ed2ac694a359d8f1153ee342ae4d85d35bfef987ad0dee5a`。
- 范围：`CausalCollector` 的事实历史与显式完整查询缓存。D1 新 runtime 链不启用 Decision/Filled map 和 trace；这些增长结构仍须通过独立 D0b storage 设计门。
- TDD：101 条事实建立 checkpoint，要求共享旧事实地址；旧 `Vec` 在 index=0 断言失败。实现后追加 67 条并恢复旧 checkpoint，顺序与 sequence 保持；7 个 collector 测试通过，0.01 秒。
- 运行：独立 `rustc --test` harness，`codegen-units=8`，测试 `--test-threads=8`，编译与执行各受 `scripts/run-with-deadline.mjs 10000` 约束。实际 `shared_history.rs` 直接包含；`causal.rs` 仅将四条子模块声明改成绝对路径，collector 逻辑和测试直接来自候选源码。`Money` 序列化 helper 从源码原段提取，公共领域类型只读旧 feature engine rlib。未重编或修改该 rlib。
- 原输出：仓库根 `.tmp/runtime-diagnostics-d1/storage/`；本目录保留红绿日志及 source/rlib manifest。首次直接 path 包含因 Rust 子模块查找目录失败，修正 harness 路径后才运行红断言；编译错误不计为红证。
- 绿测试二进制 SHA256：`f465c78289b5c8f2013040b7448c46c6a93b44e56f509bcf553c7a376be0778b`。
- 独立复核：`sol61_branch_task_inventory` 已独立核对完整单文件 diff、源码与 rlib hash，结论 APPROVED，仅此 storage 中间切片。
- 限制：这是 storage 层证据，尚未验证当前真实 crate 编译、runtime 生命周期、Protocol 跨 session checkpoint/save/hash 或 feature off。D1 不单独合入 main，不代表原 27 integration / 16 red 已修复。
- 开销：普通 append 只复制至多一个 32 条 tail chunk，checkpoint 共享事实；显式 `facts()` 首次全量查询为 O(history) 并分配副本，同版本复查 O(1)。后续 append 回收该副本也可能 O(history)，不宣称逐 tick 全量查询为 O(delta)。
