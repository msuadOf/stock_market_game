# C2-A 实施证据

## 当前状态

纯 C2-A 实现候选已冻结、13/13 实际绿；等待未实施 Sol 独立复核，尚不宣称完成。设计批准不是实现通过。

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

## 实现候选与实际绿证

- 红代码提交 `a068389e`，红原件提交 `0fc0afb0`。
- 实现提交 `3968ad17`；仅context实现110行增量，测试正文与红提交逐字相同。
- `product.patch` 是从 C2-A 增量基到实现提交的全部产品diff；reviewer还应读取整个分支增量，含工作证据。
- 产品新模块250行、独立测试668行，session.rs精确仅加一条mod；C1a未改。

```sh
node scripts/run-with-deadline.mjs 10000 -- node agents/branch-completion/mixed-calendar-c2a/run-validation.mjs compile green
node scripts/run-with-deadline.mjs 10000 -- node agents/branch-completion/mixed-calendar-c2a/run-validation.mjs cases green
```

实际轻编译731ms，8并发的13个case全部exit0，整批shell wall约653ms。每case/整批外部deadline都是10000ms。红绿binary、依赖rlib、harness和完整产品源码SHA见source-artifact-manifest.json；编译前后和执行前后均验证源不漂移。CPU/线程采样在各compile manifest；这些值是实际瞬时采样，不冒称持续满核。

Source不提供Clone，owner固定Arc::ptr_eq；receiver按owner→完整双向StockId/exchange→date→layout/before逐字段检查，错误携带expected/actual。tick先委派C1a验证before，仅Trading可checked构造下一拍，after再次C1a投影校验，civil date始终留在day。没有重复日历计数或分钟映射。

新增文件rustfmt --check与git diff --check均通过。尝试对既有session.rs做skip_children rustfmt --check为exit1，大量既有排序/换行问题；依赖基同命令也exit1。checks.json保留两份原输出；未扩范围格式化既有文件，程序化核对session.rs相对基线精确仅增加一条mod。

## 已测与未测

本次13类测试直接证明同owner/异owner、完整bindings身份、receiver各字段和错误次序、mixed首末拍及竞价中拍、复市本股ordinal/分钟、closed/TradingCompleted静点、输入错误及合法最大tick的checked溢出。UnknownStock沿真实C1a结果；C11构造校验本已由C1a支持，初红为绿，如实保留。初stub对静点拒绝的分支也已正确，C09初红发生在合法末拍不能构造，不能把它称为静点分支的初红。其他被前一个断言遮住的后续分支仅有最终绿证，不逐分支夸称初红覆盖。

未执行完整engine crate/Cargo、World/driver/group/desk、pending/资源/receipt原子性、native/WASM生产路径、live/cache、保存/恢复、NPC计划和宿主transport。C2-A只是必要纯substrate，mixed生产与C2-C/C4/C5仍另审。
