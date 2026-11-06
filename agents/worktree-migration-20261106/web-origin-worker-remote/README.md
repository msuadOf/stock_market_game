# Web origin 历史测试记录

本目录保存 2026-10-29 的两个 worktree 中 `agents/perf-intraday-independent/resume-2026-10-29/web-origin-regression/` 下的未跟踪及 ignored 普通文件。`worker/` 和 `remote/` 分别对应 `web-origin-worker-2026-10-29` 和 `web-origin-remote-2026-10-29`。未归档 symlink，包括 `node_modules`；源文件未修改。

`manifest.json` 逐文件列出源路径、归档相对路径和各自 SHA-256。归档将本机工作区路径替换为 `<WORKSPACE>`，将本机用户目录替换为 `<HOME>`；因此源和归档的摘要可能不同。日志内容为历史证据，并非当前版本的验证结果。

- `worker-origin` 的 red 和 green 均为 exit 1；green 输出仍有失败，不能视为通过。
- `worker-company-teardown` 的 focused 为 exit 0；file 为 exit 1，记录为 13 tests 中 9 pass、4 fail。`red-reference.txt` 明确记录所引用的更早红测输出缺失。
- `remote` 的 red、green 和 green-rerun 均为 exit 1，stderr 均记录 10000ms deadline 耗尽及进程树终止请求；不能把 green 名称解释为通过。
- `tauri-startup/green.log` 记录 51 tests、51 pass；该目录没有 exit 文件，不能据此推断整个命令的退出状态。red 日志包含失败输出。

本归档只保存历史输出，不改动交易规则或游戏实现，也不补造缺失证据。
