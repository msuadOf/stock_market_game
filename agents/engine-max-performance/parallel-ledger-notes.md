# immutable ledger 行校验并行记录

## 范围

将大 ledger 的必要 conservation 方程和 evidence 对行校验切为连续 64 行的 Rayon chunks；不足 512 行时保持串行。每个 chunk 内仍按原序执行，chunk 结果由 indexed parallel iterator 按源序返回，调用方按源序取首个错误。conservation 的 checked 全局求和仍逐行按原序进行，故早期资源溢出与后续行错误保持原来的先后。短 ledger 保留原有无 row-vector 的路径。

`validate_ledger_evidence_uncached` 中 live/terminal 零资源谓词继续走原 `any` 扫描。较重的 conservation 方程、`Envelope::validate`、重叠检查和 audit/conservation 查表才进入并行行检查。全量 conservation 查表在行 closure 内完成，`ConservationRow` 仅作为临时验证对象；返回值仅保留 Copy 资源摘要，不长期保存克隆的 `EnvelopeKey`。

## TDD 与验证

- 红灯：新增 4096 行线程观测测试后运行 `CARGO_BUILD_JOBS=16 CARGO_TARGET_DIR=.tmp/parallel-ledger-target cargo test -p engine --lib conservation_rows_execute_on_multiple_rayon_workers`，失败在 worker 集合只有一个线程，符合预期。
- 绿灯：同一测试现通过，观察到 4 线程 pool 中多于一个真实 Rayon worker 执行行校验。
- 边界：新增 4096 行、两个错误行的 helper 测试，确认按源序选出 `row 127`；511 行测试确认短输入只在调用它的 Rayon worker 上执行。
- `CARGO_BUILD_JOBS=16 CARGO_TARGET_DIR=.tmp/parallel-ledger-target cargo test -p engine --lib ledger_conservation_tests`：5 passed，case 总用时 0.04s。
- `CARGO_BUILD_JOBS=16 CARGO_TARGET_DIR=.tmp/parallel-ledger-target cargo test -p engine --lib ledger_`：63 passed，case 总用时 0.36s。
- `git diff --check` 通过。全仓库 `cargo fmt --all -- --check` 会报告大量既有文件格式差异；本次三个 Rust 文件单独经 `rustfmt --edition 2024` 格式化。
- 未运行 performance benchmark；性能收益和 16 核负载比例需由主 agent 使用独立基准确认。

## 语义边界

改动不触及交易制度、费用规则、委托顺序或 A 股领域语义。全局资源方程的 checked `ResVec` 合并顺序不变；错误选择按 ledger 的 BTree 行序保持确定。Weak identity evidence cache 与命中条件未改动。完整 diff 仍需由未实施者独立复核。
