# std-thread 收官段独立复核

范围：`337c82cc..176f4ec8`，逐一核对 `4d8960bd`、`6998a0bb`、`114e7e6b`、`39d0737a`、`176f4ec8` 的实际 diff。本复核未实施这五个提交，未修改实现。

结论：**收官段不可冻结**。以下阻断包括新回归、删除后未清理的消费面，以及本次编码域全消费面门禁仍未解决的既有缺口；分别注明，不混称为本批新引入。

## 阻断发现

1. **阻断 | packages/engine/src/runtime/desk.rs:927 | 正常取号没有 32 位守卫。**
   `place` 仍执行 `self.next_serial += 1`；只有 `reject` 改成 checked。`group.rs:331` 合成时却只保留低 32 位。恢复种子还会经 `desk.rs:2435/2460` 无域校验地安装。短 fixture 将合法可恢复档的 `book_next_sequences[0]` 设为 `0xFFFF_FFFF`，连续三拍正常申报全部成功，serial 依次为 `4294967295/4294967296/4294967297`。因此“低 32 位恒等、无先增长后截断”的前提不成立。需同时覆盖正常、拒收、恢复/种子入口及边界测试。证据：`probe-serial.log`、`probe.rs`。提交消息所称“两处取号 checked”与实际 diff 不符。

2. **阻断 | packages/engine/src/runtime/event_bridge.rs:213 | 编码消费面仍分裂，正常两股档不能恢复。**
   `event_bridge.rs:213/222` 的撤单/受理事件和 `session.rs:3355` 的存档订单仍用 `OrderId(*serial)`；group、drive 的计划/经历身份已用 `(stock<<32)|(serial&0xFFFF_FFFF)`。两股各挂首单，事件和存档均产生 `OrderId(1)`，随后的 `GameSession::restore` 返回 `InvalidSave(2162, [1])`。这些原始 serial 投影在 `4d8960bd^` 已存在，**不是本批新回归**，但本次“编码全消费面一致性”门禁未达成，单股/第 33 股单笔测试无法证明全局身份唯一。证据：`probe-two-stock.log`。修复投影时必须同步 restore 种子解码，保留 `seq` 的本地 serial 含义。

3. **阻断 | packages/engine/src/runtime/drive.rs:1323 | 日结事件数组顺序与 seq 顺序相反。**
   `collect_drained_events` 先给撤单分配 seq，再给竞价分配；`failure.rs:920/939` 却先输出竞价、再分配并输出 DayBoundary、最后输出早已编号的撤单。20 拍、开盘窗 3 拍、收盘窗 2 拍，t19 挂一笔无对手盘买单，t20 实际输出 `AuctionTick(22) → AuctionCompleted(23) → DayBoundary(24) → OrderCanceled(21)`。新增注释承诺的事件序必须与编号分配一并实现，不能只移动已编号的事件。证据：`probe-seq.log`。

4. **阻断 | packages/engine/src/runtime/world.rs:1766 | 新增 drained_auction 没有纳入 Abort 回滚。**
   Drain 回执先 append 世界级缓冲，后面的日结候选校验失败调用 `abort_tick`，但该函数只向 worker 广播，不清理/截断世界缓冲。4 拍空簿 fixture 注入日结候选失败后，`drained_auction.len()==2`；改回通过校验器重试后变为 4，失败尝试的 AuctionTick/Completed 将重复上行。需按本次日结起点回滚缓冲，保留可能已有的成功事实。证据：`probe-world.log`、`probe-world.rs`。

5. **阻断 | scripts/frontend-build-plan.mjs:17 | wasm 构建消费面仍要求已退役的线程契约。**
   正式构建仍固定 nightly、显式传 `-Z build-std=panic_abort,std`，并在 :19/:22 两次执行 `check-wasm-threading.mjs`；后者 :6 起硬要求 `shared:true`、共享 memory import、`thread_stack_size` 等 rayon 线程 glue。删除原子链接链后，正常串行 glue 无法通过此检查。`frontend-build.test.mjs:15` 仍断言 build-std/nightly，故其通过不能证明 A10 完成。需同步正式构建与测试契约。

6. **阻断 | scripts/package-static-web.mjs:23 | Pages 隔离文件删除后仍有生产消费者。**
   静态打包仍强读 `pages-isolation.js`，而 `39d0737a` 删除了唯一 public 文件；干净新构建缺该文件时打包必报 ENOENT。`scripts/pages-isolation.test.mjs:6/67` 仍加载两个已删文件，现态 4/4 失败；`scripts/smoke-pages.mjs:67/76/79` 仍要求跨源隔离、rayon Worker 数，`scripts/performance/market-ui-report.mjs:388` 也仍强制隔离。需同步退役这些消费者；`worker-host.ts:172` 和 Web README 还留有旧报错/说明。

7. **阻断 | .cargo/config.toml:8 | 空数组没有屏蔽上级 rustflags，覆盖说明不实。**
   在指定工作区实际执行 `cargo +nightly-2026-09-05 -Z unstable-options config get target.wasm32-unknown-unknown.rustflags`，仍得到上级主仓 `+atomics`、`-Z llvm-target-feature`、`--shared-memory`、TLS exports 全套参数；查询 `unstable.build-std` 仍是 `["panic_abort","std"]`。Cargo 数组合并使 `[]` 不能清空上级值。提交消息中的 `RUSTFLAGS=""` 验证主动绕开了此问题，不能证明默认配置覆盖成功。此项在当前指定 worktree 实际影响构建，不只是注释问题。证据：`effective-cargo-config.txt`。

8. **阻断 | scripts/simulation/run-escrow-verification-matrix.mjs:79 | 性能报告生产者和消费者契约未同步。**
   matrix 删除 `rayon_registry_capacity_samples` 并通过 exactKeys 拒绝该键，但仍在用的 `escrow-performance-harness.mjs:317` 每份样本都产此键。“唯一生产者随端点删除”的 :93 注释不实。matrix 短测实际 14 过、3 败，正常报告被拒为 `before performance sample fields are incomplete or unexpected`。此外 `prepare-escrow-performance-config.mjs:126/132` 仍构建已删除的 `escrow_performance_endpoint`，性能准备链也已断开。需为端点找到替代并全链同步，或显式整体退役对应工具；不能只删校验键。证据：`matrix.log`。

9. **阻断 | packages/engine/examples/escrow_verification_harness/committed.rs:75 | C 桶验证预算注入被断开，报告仍使用外层预算。**
   `execute` 仍按预算建 ComputeContext 并把其 workers 写入产物，修改后 `capture_runtime` 不再接收该 context，`initial_session_with_setup:469` 改调 `ProtocolSession::new`，内部绑定 `shared_default`。外层 `compute.install` 不会替换会话持有的 Arc。短 probe 实测请求预算 1，session compute workers=128、`Arc::ptr_eq=false`。因此不能继续把外层配置值当作会话实际绑定预算，且 C 桶测试/验证注入能力没有在 harness 全链保住。此处并不声称 runtime 启动了 128 个 ComputeContext worker；结论仅限于注入绑定丢失与报告含义失真。证据：`probe-context.log`。

## 建议

- **建议 | packages/engine/src/runtime/desk.rs:1862 | opening_cleared 的 before 值不在 undo 内。** `MarketSnapshot` 无此字段，`snapshot_market` 在 `roll_day` 之后执行，`rollback:596` 无条件写 false，`state_hash` 也不覆盖该标志。因此不能声称该字段已由 Market undo 恢复。未额外复现该差异导致的用户可见错误，故列建议；应补显式快照/回滚测试，或说明重算为何足以维持行为。
- **建议 | packages/engine/tests/session.rs:4579 | 采样注释数值错误。** seed 196 的 SplitMix64 首抽为 `0.004247391982158399`，并非注释的 `<0.004`；当前 diagnostics 用例通过，修正文案即可。
- **建议 | apps/server/src/actor/fatal_tests.rs:59 | capacity 测试名和断言不一致。** `new_session_capacity_limit_still_guards_registration` 只验证一次成功登记，不触发 Capacity；注释中“MAX_SESSIONS 大故不可构造”的理由不足，模块内可设置计数后验证拒绝。可改名如实描述成功路径，或补实际失败断言。
- **建议 | packages/engine/src/session/failure.rs:1564 | 取证日志混入正式测试路径。** 本批新增的两处 `PROBE` eprintln（另在 `notices.rs:114`）应清理或说明长期用途。

## 无碍与范围核对

- group 合成/掩码、drive 身份核/撤单/经历合成/恢复 seed 掩码、decision_chain 测试适配、session next_order_id 下界确实均调用新 helper；不是这些调用点内部出现了不同位运算。遗漏的是上面单列的事件和 save 原始 serial 投影。
- 四个删除文件为 `apps/web/public/pages-isolation.js`、`apps/web/src/pages-isolation.ts`、`packages/engine/examples/escrow_performance_endpoint.rs`、`packages/engine/examples/max_speed_probe.rs`，均在 A 桶文件类别内。没有发现 B 桶 Echo/镜像安装/同拍双写链误删。删除文件合规不代表其消费面已经清理。
- C 桶 `runtime/mod.rs` 的 wasm 恒 false、`ENGINE_STEP_RUNTIME=0`、compute.rs 核心、GameSession 的 new/restore_with_compute、rebind_compute_for_verification 仍保留；harness 的注入绑定问题另列阻断。
- `runtime_information_authority` 初始化 false、driver 构造置 true、clone_for_shadow 解构/复制、commit_from 解构/赋值、hash 显式排除全部核对完整；clone_for_tick_shadow 的 driver=None 不再绕过投递门。没有发现字段透传遗漏。
- 日结清算方向与仓内 `docs/trading-rules.md:35/74` 的“收盘集合竞价后余单失效”基线一致；没有改清算选价算法、单位或板块差异。本次未新增交易所联网取证，沿用仓内已有官方来源及其适用日期，不扩张为现行全部规则已重新核查的声明。
- 读取 `/tmp/forensic-baseline` 与 `/tmp/forensic-final` 的全部 60 份 JSON，events 60/60 相等、snapshot 60/60 相等；save 差异仅 `book_next_sequences.0` 0→1（60 份）、新增 `desk_mirrored_resting`（60 份）、`runtime_state.book_public_cutoff_sequences.0` 0→1（8 份），与三类表示差注释吻合。fixture 为股票 600001/StockId(0)，auction_ticks=0/3/6，各 20 拍；末拍 t19 对应 tick20。**这是已有取证文件的独立逐字段核查，不是本轮重新构建旧 producer，也不证明多股/有余单/失败重试场景等价。** 比较结果在 `forensic-comparison.json`。
- 预算分量删除的 run-full-regression 测试仍保留预算等式；抽取的 CPU 分配、并发独立 deadline、非法清单三例通过。未发现把 10s deadline 放宽的改动。性能 matrix 的生产/消费不同步另列阻断。

## 验证与局限

- 当前 engine lib 以 `CARGO_BUILD_JOBS=16`、`cargo build -p engine --lib --offline -j16` 检查构建，进程外 300000ms supervisor；增量构建完成，输出在 `build.log`。随后独立 probe 通过 `rustc --extern engine=target/debug/libengine.rlib` 链接该库，未修改 src/tests 实现。
- 普通 probe 均由进程外 10000ms deadline 约束；独立 probe/测试并发执行，Rust test binary 使用 `--test-threads=4`。运行期间没有分钟级单核任务。
- 现有预构建二进制：step_skeleton 7/7；weekend publications 2/2；diagnostics 1/1；scale_restore 重复 id 1/1。这里只声称该二进制结果，不声称重建并完成全量回归。
- Node：Pages 现态 0/4、matrix 14/17；frontend-build 与 static packaging 的现有短测通过，但前者仍锚旧线程契约、后者 fixture 人工生成了已删的隔离文件。通过不能覆盖上述正式链缺口。
- 完整的 run-full-regression/baseline-run 脚本测试尝试受到沙箱写权限限制：其 workspace-paths 指向只读的主仓 `.tmp`，出现 EROFS/ENOENT。未把这些环境失败计为代码缺陷，也未宣称三文件 123/0 已复现。escrow-performance-harness 一例 `/tmp` containment 失败同样不计入本批发现。

## 突变验证及还原

仅临时恢复 `39d0737a` 删除的两个 Pages 文件的父提交原字节，运行原有 Pages 测试；无 git revert、无修改提交历史或 index。

1. HEAD：4 例失败，缺已删文件。
2. 回退删除：4/4 通过。
3. 删除临时恢复文件、回到 HEAD：4 例再次失败。
4. `git diff --exit-code HEAD` 返回 0。

原始三轮输出：`mutation-head.log`、`mutation-rollback-pages-deletion.log`、`mutation-restored-head.log`；机器结果：`mutation-results.json`。开始时已存在的 v5-p1-final/v5-repair-recheck 未跟踪产物原样保留，本轮新增文件仅在此工作记录目录。未声称整个包含未跟踪产物的工作区为空。
