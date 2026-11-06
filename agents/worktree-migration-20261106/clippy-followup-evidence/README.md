# Clippy 后续工作证据归档

本目录保存四棵 worktree 的未提交 tracked diff 与全部未跟踪交接记录，是历史证据，不是产品补丁，不应直接应用。四棵源 worktree 的 HEAD 均为 `50d493701aa0fbb7d62563855a84dc07c89c29b0`；归档提交以 `ed035a87bacacfd0fe7b108a3121c7a55541738f` 为基线。每棵目录的 `worktree.diff` 由 `git diff --binary HEAD --` 采集；`untracked/` 保留源相对路径。文件哈希见 [SHA256.md](SHA256.md)。

旧 `diagnostics` 在 `inventory_sources.rs` 和 `player_company_command_candidates.rs` 把 `!is_some_and(...)` 改为 `!is_none_or(...)`，反转了两处判断；`diagnostics-rework` 删除外层 `!`，修正该反转。归档不证明这些改动适合当前版本。旧记录未经本次测试或独立代码审查，不能视为测试或审查通过；本次仅验证证据完整性。该归档不改变 A 股交易行为，历史 diff 可能影响领域语义，未来采用前须基于现行官方规则和完整测试独立评估。

对内容做了凭证关键词和本机路径扫描，发现 args-stream 的 READY.md 中有本机仓库绝对路径，已统一替换为 `<REPO_ROOT>`；未发现凭证关键词命中。扫描是启发式检查，不构成不存在敏感信息的保证。patch 原字节保留，交接记录仅替换上述路径；原始及归档哈希均已登记。

`git diff --cached --check` 对原始 patch 的上下文空白行报告 trailing whitespace；为保持证据字节不修改这些行。该检查不通过的范围仅为 `args-leaf` 与 `args-stream` 的原始 patch 副本。
