# R4 未提交内容保全（2026-11-06）

源 worktree 为 `.worktree/company-r4-production`，HEAD 为 `4f9b481147313e0ac81457a09299f48b1ff8aee0`；对照 main worktree HEAD 为 `b0ed6586628cea2a5c21782b7a5d817711316098`。本目录只是历史快照，不接入产品模块，也不宣称源码符合现行 A 股规则或可以运行。

`snapshot/` 保存未跟踪的 `admin_stage_evidence.rs`、对应 fixture 和 red1/green1 源码快照，保留原始字节。原跟踪文件 `run-clock-fixture.mjs` 的 diff 保存为文本，绝对工作区路径替换为 `<WORKSPACE_ROOT>`，因此不可直接应用；原始 diff 的 SHA-256 为 `59bc02cea761e9b9a0852ac09cdc9de1b06ab187a5c398c483587e03582dd8b1`，脱敏版本为 `31ad03482f5158886b2f8c9a45b64f476de5e2e6b33892d1a50c0ff88687f14d`。

`inventory.json` 按源 worktree 相对路径记录未跟踪与 ignored 的普通文件分类、字节数及 SHA-256，共 2125 个、281834743 字节；不包含 tracked 文件、符号链接或目录，不保证列出后仍未变化。该清单包括未复制的历史证据，不能替代原始文件。源工作区中约 274 MiB 的数据包含大 tar、压缩包及二进制文件；未对其内部逐项审查敏感信息或完整性，故未将其整体提交。其余历史证据也只列清单，未复制，保全范围不完整；须在源工作区保留这些文件并单独评估后续归档。

已检查本次保留的源码快照无常见密钥标记和绝对路径；这是有限的文本搜索，不构成完整敏感信息审计。未运行历史测试：这些文件是孤立快照，原 runner 依赖外部构建产物、固定工作区路径和历史绑定。历史 red/green 标签仅表示文件名，不代表本次执行结果。未更改源 worktree 或 main worktree。

已逐文件核对现有快照 `agents/branch-convergence-2026-11-02/last-two-ref-dispose-2026-11-03/before/company-r4-production/content/`：清单中 2125 个文件全部存在且 SHA-256 相同，无缺失或差异。该快照位于本机根工作区的未跟踪目录，不属于本提交或 main 的 Git 对象；不能假定 clone 后可用，迁移前仍需另行完成敏感内容审查和备份。上述比对只覆盖普通文件，不覆盖 tracked 修改、目录与符号链接。
