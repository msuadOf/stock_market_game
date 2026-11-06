# D2 retained 历史证据归档

这两份文件原位于 `.worktree/runtime-diagnostics-d2-retained-test/agents/branch-completion/runtime-diagnostics-d0b/d2-retained-test/`，按原始字节保留：

- `handoff.md`：SHA-256 `ba9f18abae6121126ce116cac0036b24f011379686f1f28713963b2982dd43c0`
- `complete.patch`：SHA-256 `0671b7f56472dfeb85772d3a1704956329ac1cc849df3f970f24d2909711a592`

交接记录说明旧实验当时未运行 `cargo`、`rustc` 或测试，也未编译，不代表测试通过。归档基线 main `f806052c2dcd5059c78c5c778f605ddf54135f3b` 已包含 `retained_tests.rs` 和 `perturb_volume_once` 接缝，接缝现位于 `runtime_mirror.rs`；这里保留历史证据，不应用补片或实现代码。

`complete.patch` 内的空白属于原始补片内容，保留原样以维护证据完整性。
