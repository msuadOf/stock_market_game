# Clippy 候选 worktree 历史归档

本归档来自原仓库 `.worktree/` 下三个同名目录，归档基线为 main 的 `136ab5d0`。每个目录的 `HEAD.txt` 记录原 worktree 的 HEAD；`combined.patch` 是 `git diff --binary HEAD --`，`staged.patch` 与 `unstaged.patch` 分别保留 index 与工作区层次，空文件表示该层无差异。`status.txt` 记录采集时状态。

`evidence/agents/` 保留原 worktree 中未跟踪的 agents 历史证据；`untracked-evidence-list.txt` 和 `ignored-evidence-list.txt` 分别列出未忽略与被 Git 忽略的文件。忽略文件均为历史证据，未发现需要排除的构建缓存。`SHA256SUMS` 逐项记录本目录其他文件的 SHA-256，使用 `sha256sum -c SHA256SUMS` 验证。

归档前对补丁和证据扫描了私钥标头及常见凭据赋值模式，未发现匹配项，因此未对内容脱敏；该有限模式扫描不保证识别所有敏感信息。未复制仓库配置或环境文件。

原候选未经本轮编译或测试，历史记录中的验证结论仅反映当时的上下文；补丁不能直接套用，应先核对原 HEAD、当前代码、交易语义及相关测试，再独立审查。此归档只保留历史材料，不改变游戏或交易规则。
