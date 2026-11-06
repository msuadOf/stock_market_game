# Root transport 未提交差异归档

源 worktree：`/data1/baiyifan/workplace/stock_market_game/.worktree/clippy-root-transport-2026-10-30`。
采集时 HEAD：`8f6d0423d206bc8fef49ed464552dc8d6c3710ac`。状态仅有
`packages/engine/src/session/plan_chain_candidates.rs` 的未暂存修改；`root-transport.patch`
由 `git diff --binary` 原样导出，SHA-256：
`b0b3d34493fc448d2ef3c99e10bf19ba6d9cf090d9970a58d972f001a9a87149`。

`main` 在核对时为 `e9d34053547c993d5c8b3308b8660b3796a5e881`，其祖先
`778e60598609ccdcf52dfa5115cba5975afdac7e` 已引入同一运输布局、调用改动和大小测试。
该提交对应文件的补丁与原补丁有一处注释差异：源补丁写 `EmptyRootCoordinator`，主线提交写
`空 PlanRootCoordinator`。因此本归档保留完整原文，不声称补丁逐字相同。此差异仅作历史证据，
不将其应用到当前代码，也不改变 A 股交易语义。

可在源 HEAD 对应的独立 checkout 使用 `git apply --check root-transport.patch` 验证补丁适用性；
此归档未验证原修改的运行行为。
