# Clippy 历史 worktree 证据归档

本目录保存 2026-11-06 从三棵 worktree 采集的证据，不将补丁作为当前实现或验证结论。三棵源树的 HEAD 均为 `50d493701aa0fbb7d62563855a84dc07c89c29b0`，归档提交基于 `a548d188ecbf065e15a4cc536738da953545eadd`。后者包含前者，但代码已有后续变化。

| 目录 | 来源 worktree | 内容 |
| --- | --- | --- |
| `args-tests` | `clippy-cfg-args-tests-2026-10-30` | 测试构造参数与断言参数调整，以及 READY 记录 |
| `fixtures` | `clippy-cfg-fixtures-2026-10-30` | test fixture 共享调整，以及交接与历史补丁 |
| `company` | `clippy-cfg-company-2026-10-30` | company 的 Clippy dead_code 处理及台账 |

各 `changes.patch` 是对应源树执行 `git diff --binary HEAD --` 的原始字节，涵盖暂存与未暂存的已跟踪文件；各目录下的 `agents/` 保留该树未跟踪文件的原相对路径及字节。`fixtures` 内的历史 `源码变更.patch` 是原始工作文件，不代替此次采集的 `changes.patch`。`SHA256SUMS` 记录这七份原始证据；在本目录运行 `sha256sum -c SHA256SUMS` 可核对。采集时逐字节比对源内容，未修改源树；按密码、令牌、密钥、授权头和 URL 等关键词检查，无命中，未脱敏。关键词检查不能证明不存在所有敏感信息。

采集时三个补丁均通过对历史 HEAD 的 Git index 应用检查，均不能直接应用于归档基线 `a548d188`；它们是彼此独立的历史改动，不能假定可以同时应用。移植前应逐项核对当前代码、重叠文件和领域语义，并重新测试及独立复核。补丁没有在本次迁移中执行或构建；原记录明确未运行 Cargo、Clippy 和测试，其所述静态检查仅为历史自述。company 台账引用的外部诊断文件不在本次指定归档范围内，不能据此验证诊断或宣称通过。归档本身不修改运行代码、交易制度或 A 股语义，也不证明历史补丁的领域正确性。
