# 第一批：收窄内部可见性

- 基线 `50d493701aa0fbb7d62563855a84dc07c89c29b0`；独占分支 `codex/clippy-cfg-20261030`。
- 红灯：既有 `clippy-50d493.stderr` 的 private_interfaces 原始失败；本批没有新增交易行为，不增加镜像实现测试。
- 6 个源码文件、9 个可见性关键词，签名参数/返回值/函数体/序列化不变；不把 PlanPersonalState、PendingTradeReceipt 或 PhaseTimingBinding 扩成公开 API。
- `step_on_context` 只被 session/failure.rs 内 `step_inner` 调用，收至 session；`RetailProjectionSeen::insert` 的非测试调用均在 pipeline 内，收至 pipeline。
- `runtime_personal_states`、两个 worker main、day_end 入口与 capture 绑定由 engine 内调用；仓库全部 app/example/integration-test 没有外部调用者。外部测试 personal_full_validation.rs 明确使用现有 public 读面替代私有个人态。
- OpenDerivationRecord 的个人态字段与其内部类型统一为 crate 可见；其余公开字段保持。
- A 股语义依据：本批仅收窄 Rust 内部访问面；沿用 docs/trading-rules.md 与 ADR-0017/0043，不改 T+1、整手、分/股单位、受理顺序、资金可见性或 JSON。无需为可见性选择新增交易制度。
- 静态检查：`git diff --check` 已通过；指定 world.rs 的 rustfmt 已执行。未启动 Cargo；尚未证明编译/Clippy 通过；待唯一集成窗口与未实施者完整 diff 独立复核。

冻结 patch 的 SHA-256：`04038debdf6f621507879dfd5ccb2f9f4fd882861208cadbdb35a6c324c3c0b2`。
