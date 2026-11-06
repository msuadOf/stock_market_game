# cfg 测试交接材料归档

本目录从三个原 worktree 的 `git ls-files --others --exclude-standard -- agents` 结果归档 6 个文件；保留各自来源目录和路径，逐文件 SHA-256 见 [SHA256.md](SHA256.md)。这些历史材料和 patch 仅供追溯，不代表已应用到当前基线，也不改变当前交易规则。公开 payload 的来源只有交接说明，没有未跟踪 patch。

本次迁移未运行 Cargo、构建或测试，也未验证 patch 可应用。原交接分别记载曾运行 rustfmt 和 `git diff --check`，并明确未运行 Cargo 或测试；诊断 fixture 的历史独立复核也注明实际测试待执行。这些均为原记录的历史陈述，并非本次验证结果。

本次对归档内容扫描了凭据关键词、授权头、私钥、环境文件、本机绝对路径及 URL，未发现命中；人工检查文件内容未发现需要脱敏的信息，故保留原文。扫描不能证明不存在所有类型的敏感信息。

本次 `git diff --cached --check` 对两个原样 patch 中的空白上下文行报告 4 处行尾空格；这是 unified diff 的上下文标记，保留原始字节。排除 patch 的检查以及关闭 `blank-at-eol` 后的完整检查均通过。
