# web-thread 上游快照

- 来源：<https://github.com/daxpedda/wasm-worker>，固定 commit `ce376d95dbdd9e7b59ac7de9c6f14090076f7865`。
- Cargo 包：`web-thread`，版本 `0.0.0`；许可 `MIT OR Apache-2.0`，原始许可见 `upstream/LICENSE-MIT` 和 `upstream/LICENSE-APACHE`。
- `upstream/` 为完整、未修改的 commit 文件快照；保留上游原始注释、文档和版权。`sha256.json` 记录各文件 SHA-256，可运行 `python3 agents/web-architecture-diagnosis/web-thread-import/verify-snapshot.py` 检查完整性。
- 该目录目前不是 workspace 成员，也未接入生产；受控 std 和 Worker 生命周期的兼容性尚未证明。准入范围见 [ADR-0045](../../docs/decisions/0045-web-thread-vendor-admission.md)。
- 本轮 Native 上游库源码直接使用固定 nightly 的 rustc 编译通过；原始 Cargo manifest 的 Native 和 WASM 检查在依赖准备阶段达到共享 deadline，均未进入编译。完整命令和限制见 [准入验证记录](../../agents/web-architecture-diagnosis/web-thread-import/validation.md)。

- 后续隔离 lib-only fixture 已完成 Native Cargo 检查和 WASM atomics、共享 memory 的 cdylib 链接；这不是原 manifest 的完整验证，也没有运行浏览器 Worker 或验证受控 std。证据与限制见上述准入验证记录。
