# C2-A 实施证据

## 当前状态

实施前行为红已取得，尚未实现；设计批准不是实现通过。

- writer：`/root/sol61_mixed_calendar_design/mixed_calendar_c2a_writer`。
- 独占树：`.worktree/mixed-calendar-c2a`，分支 `feat/engine-mixed-calendar-c2a`。
- 授权 main 基：`3b535043860117a4e82122c3a8cc38564307b94b`。
- C1a 原依赖提交：`fc19fcb2392544405eb682cd3b27ac36b48580e3`、`4556ecfe20c0febacef41033a3fd93446447f686`、`90196a3b5dfb7db73271cde83e6b5bfebf1a82c4`、`7994df7875477024a31772cd997fc1dcebe9c010`。
- 依赖精确叠加无冲突，对应本树 `1ead4648`、`c0681d3b`、`c95ae406`、`b786d684`；C2-A 增量基为 `b786d684fc5493301b79496b42781a700c839332`。
- 冻结设计 SHA-256：`3f9224ed7fe63168a55eab32219fbdc591655ffe20cc7a1cfc9022fb20e15845`。实际重读核对一致。
- C1a 模块 SHA-256：`173c0633b3489a852612458832046f3106e21b28c0c571d6f2ede90f8e33caf5`。

## 实际红证

先写完整行为测试与可编译 stub。Source 的 new/day 最小复用 C1a；day/tick 接收校验 stub 放行，tick stub 明确返回 MarketTickRequiresTrading。没有先写最终实现再回填红证。`red-module.rs` 保存原始 stub，测试源码在红提交中冻结。

命令均从本 worktree 根执行：

```sh
node scripts/run-with-deadline.mjs 10000 -- node agents/branch-completion/mixed-calendar-c2a/run-validation.mjs compile red-exit
node scripts/run-with-deadline.mjs 10000 -- node agents/branch-completion/mixed-calendar-c2a/run-validation.mjs cases red-exit
```

compile 成功，耗时 756ms，codegen-units=16；采样记录真实 rustc/linker CPU 与线程。cases 总 shell wall 约1.143s；13 类用例8并发，每case单独进程树10000ms，整批也由独立外部10000ms supervisor约束。12个case因 stub 行为与合同不符返回真实 child exit101；C11复用C1a的构造输入校验在stub即通过，不能将它称为初红覆盖。

初次 runner 将 runBoundedCommand 成功无返回exitCode误读为undefined；其原件以 `initial-runner-*` 留存。修复后 `red` 成功编译/行为失败，但wrapper将child101归一为1；随后保留该原件，修正提取child实际exit为101并取得 `red-exit` 完整红证。所有变化都在实现之前，测试正文未改变。

## 依赖与证据边界

root 2026-10-26 明确授权本轻量 harness 只读复用 C1a engine rlib；原依赖路径与SHA见每次compile manifest。其engine rlib来自C1a red-build基791858d4，仅提供真实Calendar/observation/StockId；本树真实C1a、C2-A源码按path重新编译，不是替身Calendar。未调用Cargo，无最终完整engine crate通过结论。

重读根ROLE/AGENTS、principles/testing/error-handling/architecture/open-questions、Git daily-workflow、Calendar与分钟/日结相关ADR和trading-rules；本片只新增纯immutable派生类型与只读receiver检查，无新制度、无生产caller、无live/cache/SaveSlot。官方重读条款、原件SHA和适用日期另见official-source-check.json。
