# Publication callback 测试合同更正（2026-10-30）

## 范围与依据

本次只改 `packages/engine/src/session/pipeline/spine_v0_contract_tests.rs` 的既有用例 `v0_publication_failure_installs_nothing_and_keeps_session_steppable`。依据 DP1：runtime 普通拍的 World 发布不可撤；最后 callback 失败时 session mirror 保留旧值并封存首 fatal，后续入口不得重跑。合同原文见 `agents/perf-intraday-independent/interface-freeze-dp1-dp4-2026-10-26.md` 的 DP1 与 `agents/perf-intraday-independent/convergence-contract.md` §2 #2/#3、不可违反项。WASM/serial 分支继续保留旧 candidate rollback、同批重放及真实 Trade 尾断言。

主仓 failed-cases 原始记录指出该测试在 callback 拒绝后再次 `step_frame()` 时仍期待成功，实际收到已保存的同一 `InvariantViolation`：`<项目根目录>/agents/branch-completion/validation-scheduler-2026-10-29/engine-unreached-41398b13-2026-10-30/failed-cases.json`，case 名为 `session::pipeline::spine_v0_contract_tests::v0_publication_failure_installs_nothing_and_keeps_session_steppable`。

## 修改与检查

保留精确测试名。WASM 分支保留 serial 候选不提交、完整状态指纹不变、重放 staged handover 后成功交付合法且含真实 `Trade` 的 frame。native 分支经正式 `step_frame_and_commit(false, ...)` dispatcher，在合法且含真实 `Trade` 的 runtime frame callback 中注入并精确比较完整 fatal；断言 hash/tick/seq/RNG/order ID/receipt 游标不变、session mirror 未安装、PublishedTick tick 与 generation 前进、World tick 留在 `before.tick + 1`、原发布槽及 authority epoch 保留、driver 仍在位，并验证后续 `step_frame`/`step`/`save` 均返回相同首错且不再改写 session 指纹。

按任务限制未运行 Cargo、rustc、Node、target 相关命令。已执行 `rustfmt --edition 2024 --emit stdout ... >/dev/null`（语法解析）与 `git diff --check`；完整 rustfmt 格式化会改写本测试之外的 3 个既有断言排版，因此净 diff 中保留其基线格式。未提交。
