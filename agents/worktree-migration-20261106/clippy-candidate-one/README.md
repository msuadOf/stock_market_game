# Clippy CFG 候选一历史证据

本目录归档 `.worktree/clippy-cfg-2026-10-30` 在采集时的状态，来源 HEAD 为 `50d493701aa0fbb7d62563855a84dc07c89c29b0`；归档基线为 main `975fe4bb7619815dba9b61ec8e2629be515cc046`。本次只保存证据，不应用候选代码，也不改变交易规则。

`staged.patch`、`unstaged.patch`、`combined.patch` 分别是暂存区相对 HEAD、工作区相对暂存区和工作区相对 HEAD 的 `git diff --binary`；空的 staged 文件表示采集时没有暂存差异。`status.txt` 和 `HEAD.txt` 保存状态与 HEAD。`source-files/` 按源仓库相对路径保存全部未跟踪候选材料、未跟踪的 `worker_context.rs` 完整副本，以及被忽略的 `candidate-1-source-manifest.json`。该 Rust 文件与 `04efdb98aec0067fefaa16504aae9974c0897f90` 对应文件字节相同，SHA-256 为 `0d687ca74031bc8868432efd72fc438b4cefaaf459410c76246175b86a96953a`。

候选资料声明当时未启动 Cargo、编译或测试，本次归档也未编译或测试候选，不能视作验证通过。main 包含候选之后的修正，不能认为与候选完全等价；例如 `runtime/mod.rs` 的 `LifecycleKind` 导出在 main 增加了 `simulation-diagnostics` 与非 wasm32 条件，原候选仅为 `cfg(test)`。恢复或使用候选前应重新核对差异及目标平台验证。

为避免暴露本机用户名和绝对路径，`candidate-1-READY.md` 中唯一的仓库绝对路径前缀替换为 `<仓库根目录>/`；替换前原文件 SHA-256 为 `a2c12339a9ea3e38a502d4f378a83ebd31288350cb6daf87a5d0bd8b8f86f5b8`。其余文件按原始字节复制。`SHA256SUMS` 列出除自身外所有归档文件的 SHA-256，包含脱敏后的副本与本说明。
