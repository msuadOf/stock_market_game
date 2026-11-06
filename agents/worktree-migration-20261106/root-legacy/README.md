# 旧工作区未提交补丁归档

`legacy-uncommitted.diff.txt` 原样保存旧 detached HEAD 工作区的历史候选改动，涉及六个已跟踪文件。它不是已验收实现，不能直接覆盖或应用到当前 `main`；后续如需采用，应先逐项检查上下文、交易语义和测试，再独立审查。

- 来源工作区：`/data1/baiyifan/workplace/stock_market_game`，来源 HEAD：`af0fdeb6c952f294e3069f031c38f4d8ad0fd6b4`。
- 来源文件：原工作区 `agents/remaining-branches-review-20261103/roster-unstaged.diff`；与采集时的 `git diff --binary` 字节一致。
- 补丁 SHA-256：`69cef156dad4d79fdcc1c2d59629067689162c90c2fc710599efbf91925f894f`。
- 本归档基于 `main` 的 `92600c3a7533a2c7188f4efe69ef522e5857ecce` 创建；这不是补丁的来源 HEAD。
- 补丁中引用的相对路径和日志仍在原工作区的对应上下文中；本归档只保存补丁本身，不声称保存了这些链接指向的完整内容。

本次只保留历史字节，不实施交易规则改动，也不将补丁中的历史状态视为当前事实。

归档文件包含 19 个由单个空格构成的 diff 上下文空行。作为新增文本运行 `git diff --check` 会将它们报告为行尾空白；为保持补丁字节及 SHA-256 不变，此处保留这些空格。README 单独通过空白检查。
