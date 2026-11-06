# fixture 批次独立源码复核

- reviewer 为父级实施员，未参与该 fixture 批的实现；完整 diff 来源为 `.worktree/clippy-cfg-fixtures-2026-10-30`，源码 diff SHA-256 `3d8be8aa21d0be8da3356e54d6226ad300c97e95ab89b7f2e7094d52296a4f02`。
- 逐个阅读全部 25 文件 diff：只有 test-only 父模块注册、consumer import 与 disclosure fixture helper 的 crate 内可见性；没有改测试/断言/fixture 的业务函数体。
- 复核加载 cfg：company 的共享 fixture 仍 `cfg(test)`；runtime 两个共享 fixture 仍 `cfg(test)`。测试导入均处在原 test 后代或 `cfg(test)` use 中。无生产公开 API 扩大。
- 复核共享状态：information_fixture::calendar_source 唯一 OnceLock 保存不可变 MarketCalendarSource；原 owner 只含 calendar/start_date/bindings，无 Mutex/Atomic 或可写共享权威。本批减少 source 实例，测试独立 World 的处理状态仍各自拥有；fixture 的 clock/world 仍引用其同源 owner。
- A 股语义：沿用现有 Calendar/公司披露 fixture，没有改变证券身份、自然日、交易日、公开信息可见时点、分/股单位或现行交易规则。无需为 import/cfg 新增制度依据。
- 必要性/最小范围：共享 fixture 只编译一份，避免 duplicate_mod 和每份未消费 helper 的 dead_code；没有删去行为能力或添加伪调用。
- 边界/跨层：未发现源码级阻断；实际编译仍必须覆盖 native test（全部 shared helper）与 wasm test cfg；未启动 Cargo，不能宣称 Clippy 或测试通过。

结论：源码复核通过，等待集成编译/Clippy/测试实际门禁。
