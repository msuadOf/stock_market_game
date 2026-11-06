# escrow harness 历史证据归档

本目录保存原独占 worktree 的全部证据（包括被 Git 忽略的 JSON）及根工作区独立终审。`worktree/` 与 `root-workspace/` 区分来源，避免重名；`manifest.json` 逐文件记录来源、归档位置、两端 SHA-256 和路径替换次数。归档中将原机器工作区绝对路径统一替换为 `<WORKSPACE>`；其余字节保持不变。因此被替换文件的 SHA 不同，原值仍可用于核对源文件。归档的脚本和历史命令只是证据，并未在迁移中执行。

历史结果以 `worktree/README.md` 和 `root-workspace/sol-review.md` 为准：原 red 为 10 通过、10 失败；attempt1 为 19 通过、1 失败；此前 GNU timeout 的绿色结果不能证明共享进程树截止，最终由项目 supervisor 补跑两分片 20 项通过。历史记录明确未运行完整 scripts、完整回归或正式性能 benchmark；本次迁移也未重跑这些验证，不能据此宣称整体验收或正式性能测量通过。

本次仅迁移证据，不改变交易制度或游戏行为；A 股语义以原项目正式文档为准。历史文件中的路径、提交状态和证据位置描述保留为当时语境，不代表迁移后的当前状态。
