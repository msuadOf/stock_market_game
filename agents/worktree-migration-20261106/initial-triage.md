# Worktree 迁移初筛概要（2026-11-06）

本概要记录本轮初筛，基于 main `5adf6a6d`。它不是全面的逐文件或交易语义审查，也不构成迁移、提交或删除文件的授权。具体状态及扫描限制见 [状态起始快照](status-snapshot.md)。

以下方向在 main 已有覆盖，初筛不应把对应工作区差异直接视为待移植功能：

| 方向 | main 中的覆盖依据 |
| --- | --- |
| 日历上限 | `3be32dbd` |
| timeline 开盘守卫 | `8e058aca`、`a1a7f290` |
| export 隔离 | 现行 supervisor |
| pending cash | `e02c1b67` |
| worker error fidelity | `cafc847d` |

main 工作树中的 `wasm_tombstone` 是未接线实验；`company-write-web` 工作区有两个 `UU` 冲突条目。其他 calendar、runtime、wasm 和 clippy 相关工作区存在多处差异，目前仅完成初筛，不能直接移植；后续须逐项确认归属、依赖、测试及大 A 语义。

根工作区约有 24 万个未跟踪项，混合历史证据与构建产物；这一估计不提供逐项分类依据。快照中的 186 个 worktree 是有状态条目或扫描错误的集合，并非全部确认为脏；其中两次 `git status` 超时，状态未知。未跟踪目录可能在状态输出中折叠。实际处理前须重新扫描并核对，不应据此删除任何内容。
