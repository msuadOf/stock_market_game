# calendar-new-due-horizon 工作树证据归档

- 来源：`.worktree/calendar-new-due-horizon-2026-10-30`，来源 HEAD 为 `70c88c7a86722a57500595791bca3a6bb98be620`。
- 归档基线：main 的 `739d869331756cd3327c4a4b9c6bb29be734aa5a`。该基线包含 `0cf09ff8`（新增到期日越界的日结来源拒绝）；来源未跟踪测试源码与该提交中的同名文件 SHA-256 相同，完整 `product.diff` 与该提交的 stable patch-id 同为 `cdafb1c410bf39743afb447ebc811d1c4e540b38`。证据仅作历史记录，不重新应用补丁。
- `worktree.diff` 是来源执行 `git diff --binary HEAD` 的原始输出，仅包含两个已跟踪文件；该命令不包含未跟踪文件。`source-agents/` 原样保存来源证据目录全部文件；`civil_due_horizon_runtime_tests.rs.txt` 另存来源未跟踪测试源码，`source-agents/product.diff` 亦包含它。
- 原始记录说明当时只运行单文件 `rustfmt` 和 `git diff --check`，未运行 Cargo、rustc 或测试 case。本次仅检查归档一致性，未运行产品测试，不推断测试通过。
- 按凭据、令牌、私钥、绝对本地路径和 URL 等模式检查全部归档内容，未发现匹配；无需脱敏，内容按原字节保存。模式检查不能证明绝无敏感信息。`git diff --cached --check` 报告两个原始补丁中的上下文空行含尾随空格；为保持证据原字节而保留。

## SHA-256

```text
9e7dafb462375dcd27c7adb1e37b03865b95dc1fb50868965ab7c7a17d4ecbb3  worktree.diff
93923606e77da42d2908301c8d8aa1b3f2dd5e88f2b22bcc429bdd0de69e2087  source-agents/implementation.md
62bd87a82a3d0a653de5a387ecc89b78c6e2e52a75cf64b646c97c4bc2b0dc75  source-agents/product.diff
99c844b5a6235105e2d9a7fb9bf5e9b34bc58c11a891da3c7640012f453d8a76  civil_due_horizon_runtime_tests.rs.txt
```
