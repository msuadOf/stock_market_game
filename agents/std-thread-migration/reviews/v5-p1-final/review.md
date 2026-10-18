# std-thread 迁移五审重送批 P1 闭合轮终复核

审查人身份：未实施本批改动的终复核 agent。工作区 HEAD 为 `b1171ac5bd712f2a436e7a230e00eef6b7920c32`；完整审查范围为 `git diff fc2145b0..b1171ac5`，包括 c342c93b 修复与 b1171ac5 归档，共六个文件。未提交变更，临时源码已还原；既有未跟踪证据未修改。

## 1. P1-1 js_safe 域验证

- 代码位置：`packages/engine/src/session.rs:3423、3428`，镜像和游标两条分支均逐项夹取 `js_safe_u64::MAX`。wire 校验位置为 `packages/engine/src/orderbook.rs:95`。
- **33 股实跑通过**：零 NPC、33 股合法局，先确认步进前可序列化，再运行一拍并确认空簿；保存得到 `next_order_id=9007199254740991`。JSON 为 49,926,572 bytes，依次完成 `serde_json::to_vec`、`serde_json::from_slice::<SaveSlot>`、`GameSession::restore`、恢复后再次保存并序列化。结果 1 passed / 0 failed，进程 wall-clock 90.751 秒。证据：[完整输出](33-stock-long.log)、[期限及 CPU 记录](33-stock-summary.json)。
- 第一轮完整往返在自设 75 秒子期限下超时，未计为通过；失败输出完整保留在 [attempt01-probes.log](attempt01-probes.log) 与 [attempt01-recheck_idle_33_stock_json.log](attempt01-recheck_idle_33_stock_json.log)。随后将该最小股数边界拆成独立长验收，进程外总期限 299 秒，child 295 秒，没有修改断言。JSON 单流编解码及同一 SaveSlot 的 restore 存在先后依赖，不能拆成并发的半条往返；独立的 31 股探针和 shutdown 突变构建与其重叠执行。
- **≤31 股域检查通过**：合法 31 股局，分别执行 cursor 分支和 mirror 分支，使用接近 48 位 serial 上限的游标；两者得到相同 floor `8725724278030305`。逐股检查夹取不改变编码，并检查 floor 严格大于该股 `serial=cursor−1` 的最大已耗身份，从而覆盖全部 `serial<cursor`。JSON 序列化通过，结果 1/0，23.501 秒。另复跑两股 `[7,11]` 游标、JSON、restore、恢复后新单及再次 restore，1/0，4.734 秒。证据：[31 股输出](final_recheck_31_stock_floor_and_wire_boundary.log)、[两股输出](two-stock-green.log)。
- **预存在缺口事实成立，但登记不完整**：同一实跑负控中，第 31/32 股身份可序列化，第 33 股 `OrderId((32<<48)|1)=9007199254740993` 被拒绝，错误为 `exceeds JavaScript's safe integer range`。`group.rs:320` 的实际 `order_id_of` 函数在 fc2145b0 与 b1171ac5 逐字节一致，证据：[编码比较](preexisting-encoding.txt)。正确范围是 **stock index≥32，即至少 33 股**，不能写为“至少 32 股”。
- `session.rs:3418` 与 c342c93b 提交说明已经承认该限制；但正式 `docs/` 与迁移计划台账未找到该项登记，归档的上轮 review 记录的是 next_order_id 阻断，不能替代 desk OrderId 预存在缺口的专门登记。实际 Git 查询保留于 [registration-search.txt](registration-search.txt)。建议在 `intentional-divergences.md` 或正式缺口文档记录触发范围、涉及的经历/计划 OrderId、目前不支持的边界；不得将此次空簿修复表述为第 33 股及以上所有成交存档均已支持。

**结论：技术修复无碍；P2 建议补齐缺口台账与准确股数表述。**

## 2. P1-2 关停 seam 突变验证

代码位置：`world.rs:335` 的 `cfg(test)` 字段、`world.rs:1892` 的广播前停顿、新测试 `group.rs:5338`。

实际在 `World::shutdown` 入口临时加入 `self.worker_shutdown.store(true, Ordering::SeqCst);`，重新用 32 jobs 编译后执行新测试：

```text
正常关停成功（停顿窗不破坏协议）: SendFailed { detail: "组 Shutdown 广播失败" }
test result: FAILED. 0 passed; 1 failed
```

退出码 101，0.137 秒。完整错误输出：[shutdown-red.log](shutdown-red.log)。这次失败确实发生在组 Shutdown 广播处，没有被 Drain 失败或其他异常替代。

随后在 finally 中按初始 bytes 还原 `world.rs` 和临时测试文件，逐字节比较成功，`git diff --exit-code` 返回 0。按恢复后的源码重新编译，原测试 1 passed / 0 failed，退出码 0，0.136 秒：[shutdown-green.log](shutdown-green.log)。[probes.log](probes.log) 记录红→还原→重编译→绿的完整顺序；[restored-sha256.json](restored-sha256.json) 保存还原文件哈希。

绿测后及全部验证结束后，再次执行 `git diff --exit-code`、`git diff --cached --exit-code`，均返回 0；[工作树输出](final-worktree-diff.log)、[暂存区输出](final-index-diff.log) 均为空。HEAD 未改变。

**结论：无碍，P1-2 闭合。**

## 3. P2 文档修改验证

`docs/trading-rules.md:274` 改写后的买腿边际口径与 `runtime/desk.rs:1178、1186、1234、1251` 一致。实际重跑 `restore_carries_true_fill_facts_and_continues_buy_with_cumulative_fees` 通过（完整回归 0.882 秒，恢复后复跑 0.851 秒）；卖腿 `seller_fee_capped_by_proceeds_in_priority_order` 也通过（0.006 秒）。证据：[fee-cases.json](fee-cases.json)、[恢复后费用测试](fees-green.log)。

本批未修改费率或交易制度；卖腿封顶仍明确属于既有游戏简化（`trading-rules.md:13、110`）。本轮核验的是代码与既有领域契约一致性，未重新联网核验现行官方费表。

上轮 P2 的另一半仍存在：`session/persistence/saved_runtime.rs:879` 仍将 desk Buy charged 解释为分笔完整 nominal 相加，且保留相应宽容校验。该代码不在本批 diff 中，建议同步过时注释并单列评估兼容校验，不要求本轮顺带修改生产逻辑。

**结论：所指定的 trading-rules 段落无碍；旧审计注释为 P2 建议项。**

## 4. 回归数字核对

实际分别构建 `cargo test -p engine --tests --no-run -j 32` 与 `cargo test -p server --tests --no-run -j 32`，避免 server 的 dev-feature 混入 engine 基线。构建输出：[engine](build-engine.jsonl)、[server](build-server.jsonl)。

长验收批次：128 可用 CPU、32 个业务 case 进程并发、ts-rs export 另以 4 进程分片；每个普通 case 独立进程，期限 9.5 秒，完整批次进程外期限 299 秒。全部 4633 个非 ignored 执行任务完成，wall-clock 47.734 秒，child CPU 1338.162 秒（平均约 28 个核），零超时。结果：[summary.json](summary.json)、[完整逐例输出](cases.jsonl)、[批次日志](regression.log)。临时探针未计入回归。

| 范围（通过/失败/忽略） | 目标 | 实际 | 判断 |
|---|---:|---:|---|
| lib，runtime=0 | 1660/0 | 1660/0，另 11 忽略 | 相符 |
| lib，runtime=1 | 1660/0 | 1660/0，另 11 忽略 | 相符 |
| engine，原声明含 runtime=1 lib 的汇总口径 | 2834/12/17 | 2834/12/17 | 相符 |
| server | 123/4/1 | 114/13/1 | 不相符 |

真正仅集成 binary 的实际数字为 **1174/12/6**；2834/12/17 包含一次 runtime=1 lib，不能把该汇总再次当成纯集成 case 数。

engine 12 败与 `intentional-divergences.md:116` 登记完全对应：

- B6：diagnostics 1、step_skeleton characterization 3、scale_restore 1、weekend publication 1。
- C6：session 默认资金 1、save_contract 的 calendar / bank ECL / industrial loan / operating due id / plan horizon 共 5。

server 实际 13 败：

- API 断言 3：`public_report_by_id_rejects_unknown_and_company_mismatched_reports`、`public_report_page_requires_the_owning_session_token`、`public_report_routes_reject_invalid_pagination_and_credentials`；均在 `api_contract.rs:399` 因响应体不是 JSON 失败，与已登记清单一致。
- WS 10：全部在 `apps/server/tests/ws.rs:62` 的 listener bind 失败，错误为 `Os { code: 1, kind: PermissionDenied, message: "Operation not permitted" }`。沙箱不允许该 bind，且本轮不能申请权限提升。这 10 项均未到达业务断言，原目标里的 1 项 WS 业务失败也被遮住，不能假定另外 9 项会通过。

相对 server 目标，实际少 9 个通过、多 9 个失败、忽略相同。所有 25 个失败的用例名、断言和完整错误输出保留于 [failures-full.txt](failures-full.txt)，未删断言、未跳过失败或将环境失败算作通过。

**结论：lib 与 engine 数字无碍；server 目标核验受环境阻断，无法确认 123/4/1。**

## 5. 总体结论

**两个 P1 的技术验证均已通过；当前不作“全部目标通过、可送六审”的无条件结论。**

- **验收证据阻断项**：必须在允许本地 listener bind 的执行环境复跑 server，并核对是否实际为 123/4/1。本轮没有发现新增 server 代码问题，但实际 114/13/1 不能替代目标结果。
- **P2 建议项**：将 stock index≥32（第 33 股起）的预存在 desk OrderId wire 缺口正式入台账；同步 `saved_runtime.rs:879` 的旧费用口径注释。未实施修复，现如实报告。
- 完整 diff 范围与修复需要一致；未发现新增撮合、整手、T+1、费率或金额单位变化。本结论以本轮实际探针与回归输出为依据，不替代未执行的 WS 业务验证。
