# 六个 worktree 的历史证据

本目录保留六个来源的工作记录、原始测试输出和源码补丁；历史结果仅代表当时记录，不能推断当前版本测试通过。失败、未启动及未执行的记录均予保留。历史材料中的交易语义和依据保持原样，本次归档不改变游戏规则。

`manifest.json.txt` 逐文件列出来源相对路径、来源原始 SHA-256、归档相对路径和归档 SHA-256。源目录逐项递归枚举，包括 Git 忽略的日志；没有排除任何源文件。为使 Git 可追踪，`.log` 和 `.json` 文件分别以 `.log.txt` 和 `.json.txt` 保存。文本内项目本地绝对根路径替换为 `<项目根目录>`；其余内容不作有意变更。因此部分归档 SHA-256 与源文件不同。源中的路径和相对链接是历史引用，不保证在归档位置有效。

`calendar-mirror-origin-oracle-2026-10-30` 的实际来源为对应 worktree 下 `agents/calendar-mirror-origin-oracle-2026-10-30/`；同名 `agents/branch-completion/` 目录不存在。其 manifest 明确记录 Cargo、rustc 和 case 均未运行。`trading-timeline-day-guard-2026-10-30` 的 lint 未启动，原因是 `oxlint` 不存在；其 red 和 green 输出均保留。`attention-civil-fixture-2026-10-30` 未运行 Cargo、Node 或 target 测试。其他验证结论以各原始记录为准。
