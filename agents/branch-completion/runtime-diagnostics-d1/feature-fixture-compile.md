# feature-only fixture 编译修正

- 当前 D1 基线与当时真实 `refs main` 同样在 `session/pipeline/plan_owner_tests.rs:614` 构造 `OrderFillSettlement` 时遗漏 feature-only `gross`。
- 真实 compiler 错误：`E0063: missing field gross in initializer of session::OrderFillSettlement`，原件保留在根 `.tmp/runtime-diagnostics-d1/check-1.stderr`；读取到的 main 原文件保留为同目录 `fixture-main-missing-gross.rs`。
- 仅补 `#[cfg(feature = "simulation-diagnostics")] gross`，值严格来自该 fixture 已登记执行价 950 分 × 100 股，使用 checked `Money::mul_shares`；不改业务实现或原断言。
- 该编译错误不能计作 D1 生命周期行为 TDD 红。修正已随真实 feature lib-test 构建通过并获完整 D1 diff 独立复核。
