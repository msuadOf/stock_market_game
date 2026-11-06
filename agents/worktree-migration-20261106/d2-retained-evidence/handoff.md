# D2 retained 边界测试补片交接

- 基线：`4a0c203227f9afacdda2e94fb9a303b56260e711`。
- 分支：`test/runtime-diagnostics-retained-boundary`。
- 实现范围：仅在 `failure.rs::assemble_retained_day_snapshot` 增加受 `test + simulation-diagnostics + native` 限定的 `perturb_volume_once` 接缝，并新增 `runtime_diagnostics_tests/retained_tests.rs`。
- 父模块声明由 D2 主 writer 在其树中单独增加：`#[cfg(all(test, feature = "simulation-diagnostics", not(target_arch = "wasm32")))] pub(in crate::session) mod retained_tests;`。
- Fixture 使用 `quote_setup(0)`，在 `GameSession::new` 前设置两拍、零收盘竞价、零 NPC；通过 `join_market` 创建真实卖方并赋予旧可卖 100 股。第一拍真实挂卖形成 prior causal，末拍买方限价买入 100 股触发 retained 日K与 minute bar 核对。
- 两个用例分别覆盖 `step_via_runtime` 和 `step_frame_and_commit_via_runtime`。hook 仅修改真实首个非空 bar 的 `volume_shares`，使用 `checked_add(1)`，记录 volume、trade_count、修改值与命中数。测试核对错误位置/文案、回滚的 causal sequence、两个账户、挂单、分钟桶与 retained 快照、健康 checkpoint poison oracle，以及重建 driver 后恰一笔 100 股 `Trade` 和无余单。
- 此补片依赖 D2 主片最终在同父模块添加声明；实现基线没有该声明和 D2 新生产恢复代码。未运行 `cargo`、`rustc` 或测试，未编译；不得视作测试通过。D2 合片后的必要首 build/run 才能确认类型和行为。
- 无 A 股制度变更：测试只断言已有 A 股 `100 股` 整手成交与账户/持仓事实，不修改产品交易逻辑。
