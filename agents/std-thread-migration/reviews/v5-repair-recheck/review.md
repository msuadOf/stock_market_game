# 阻断项

1. **P1：新的 next_order_id 下界使 33 股及以上的合法空簿局无法序列化。**
   `session.rs:3419、3422` 把每股游标编码为 `(stock << 48) | cursor`，但 `SaveSlot.next_order_id` 在 `session.rs:542` 仍使用 `js_safe_u64`，其上限为 `orderbook.rs:95` 的 `9007199254740991`。
   实际临时用例构造 33 股、零 NPC 的合法局：步进前 SaveSlot 可转 JSON；无成交地运行一拍后，空簿存档的 `next_order_id=9007199254740993`，`serde_json::to_value(&save)` 报 `exceeds JavaScript's safe integer range`。
   复现完成用时 23.38 秒，最小触发规模为 33 股，按边界长验收单列，进程外 deadline 为 299 秒。证据：`probe-33-stocks-long.log`；临时用例源码保存在 `run_probes.py` 的 `PROBES` 中。该探针与 charged 突变复用测试构建，但 next_order_id 代码保持 HEAD 原样，且无成交，不执行被突变的费用结算分支。
   影响包括需要 JSON 的存档、宿主传输和恢复链。不能只证明 Rust u64 内编码成立，必须同时闭合 wire 数值域。

2. **P1：shutdown 仍缺真实 World 控制窗口的确定性回归，局限登记不足以闭合该门禁。**
   `group.rs:5212` 创建的标记始终为 false，`5265` 的等待与 `5291` 的断言都发生在直接调用 `group_main` 的测试中，没有执行 `World::shutdown`。该测试不能检查 World 是否错误置位。
   `group.rs:5302` 的 World 测试没有控制 Drain 窗口。实际把 `world.rs:1852` 前重新插入 `self.worker_shutdown.store(true, Ordering::SeqCst)` 后，循环级测试仍通过，World 测试重复 32 次全部通过（8 并发）。证据：`mutation-shutdown-loop.log`、`mutation-shutdown-summary.json` 及逐次日志。
   `group.rs:5333` 所称“标记置位”的确定性锚没有在该模块实际执行。这里指出的是测试门禁缺口，当前 HEAD 的正常 shutdown 实现没有前置标记。

# 建议项

1. **P2：同步费用文档与旧审计注释。** `docs/trading-rules.md:274` 仍把买腿 charged 描述为分笔 nominal 相加；`session/persistence/saved_runtime.rs:879` 也仍按该口径解释 Buy 宽容校验。这与此次边际累计修复及 `envelope_projection.rs:308` 的硬等契约不一致。应同步文字，并评估收掉不再需要的分段 Buy 校验分支。

# 无碍项

1. **adopt purse 两形态一致。** `drive.rs:2447、2542` 均从已验证 envelope 的 live cash 读取 purse，并迁入真实累计额、数量和费用。旧簿投影以 `envelope_projection.rs:248` 的 `buy_order_reservation` 计算占用，恢复侧 `saved_runtime.rs:609` 校验完整证据。新增 crafted 旧簿档用例和三段 restore 链在 runtime 模式均通过。
2. **买腿 charged 修复正确。** `desk.rs:1178、1186` 的 delta 与 purse 实扣使用相同费用函数；`1234、1251` 两个买腿分支按 delta 累加，telescoping 等于累计 nominal。卖腿仍使用 `1215、1221` 的累计 nominal 与实收封顶分配，两个 Sell 分支没有改动。卖腿封顶测试及主动卖、被动买、加仓投影矩阵均通过。
3. **三段链保留价优判别并加强中间静点。** `saved_runtime_tests.rs:1966` 验证真实成交量，`1973` 保留首段 99000 分的断言，`1979` 在各段恢复。卖方 grant 400 与总计卖出 300 相容。
4. **较小 stock 域的游标下界与新单续行通过。** 临时两股用例设置游标 `[7,11]`，确认保存下界为 `(1<<48)|11`，JSON、restore、恢复后新单及再次 restore 均通过。证据：`probe-two-stock-floor.log`。此结论不覆盖阻断项 1 的 wire 上限。
5. 完整 diff 为指定的 `d9801d98..fc2145b0`。未新增依赖、未改费用费率或交易制度；累计费用结论针对项目既有费用契约，不冒称重新联网核验官方费表。B/C 分类文档更新与五审清单一致。

# Shutdown 处置判断

**必须产线 seam：需要在真实 World::shutdown 路径设置最小测试观察/控制点，可仅在 cfg(test) 构建中存在。**

撤销“shutdown 返回后标记为 false”的断言是正确的：`shutdown(self)` 消费所有权，成功收齐 Final、join 后仍会执行 `world.rs:2016` 的 Drop，`2017` 可以合法置位。

但所有权消费不妨碍在函数内部、Drop 前观察协议窗口；当前没有 Drain 延迟 seam 只说明尚未提供注入点。应控制真实 Drain/Shutdown 窗口，检查标记在该窗口未置位，并保留在途资源落账断言；重引前置标记必须确定失败。现有机制测试与概率性 World 日志测试不能替代这一要求。无需新增面向用户的 API 或改变 release 交易行为。

# 突变验证抽查

- 将 taker/maker 买腿的 commission、transfer 两处分支同时回退为每笔完整 nominal：三段链在 leg2 恢复确定失败，错误为 `fault[2220] [0, 0, 2]`，0.64 秒。证据：`mutation-charged-red.log`。
- 另抽查 World 前置永久标记：循环级测试通过，World 测试 32/32 通过，证实测试漏检。
- `run_probes.py` 在 finally 中逐字节还原 `desk.rs`、`world.rs`、`saved_runtime_tests.rs`；SHA-256 记录为 `restored-sha256.json`。临时探针不计入回归数字。
- 还原后重新编译，三段链 1/0（0.86 秒）、adopt 1/0（0.65 秒）、shutdown 两例 2/0（0.13 秒）。`git diff --exit-code` 成功，HEAD 仍为 `fc2145b0`，仅新增审查证据目录。

# 回归数字

编译分别运行 `cargo test -p engine --tests --no-run -j 32` 与 server 构建；使用各自 feature 集，避免合并编译的 dev-feature 混入 engine 基线。lib 与集成的相同测试二进制按 case 分片执行，业务测试 32 并发；普通 case 进程外 deadline 为 9.5 秒，完整批次 deadline 为 299 秒。完整批次实际 208.45 秒，子进程累计 CPU 1328.477 秒。

ts-rs export 递归生成共享依赖文件；并发超时后对失败项隔离重试，仍保留超时结果。原始记录为 `cases.jsonl`，重试记录为 `export-retries.jsonl`，最终汇总为 `final-summary.json`。未使用跨 feature 的 preliminary 结果作为结论。

| 范围 | 声明（通过/失败/忽略） | 实际通过 | 断言或环境失败 | 超时未完成 | 忽略 |
|---|---|---:|---:|---:|---:|
| lib，runtime=0 | 1659/0（另有 11 忽略） | 1653 | 0 | 6 | 11 |
| lib，runtime=1 | 1659/0（另有 11 忽略） | 1651 | 0 | 8 | 11 |
| engine，含 runtime lib 的原声明口径 | 2833/12/17 | 2825 | 12 | 8 | 17 |
| server | 123/4/1 | 114 | 13 | 0 | 1 |

engine 的 12 个断言失败完全对应 **B6/C6**：diagnostics 1、characterization hash 3、scale_restore 1、weekend 1；默认资金 1、save_contract 5。未出现额外业务断言失败。

server 为已登记 API 断言失败 3 个，加上 `apps/server/tests/ws.rs:62` 本地 bind 被沙箱拒绝的 10 个；原声明中那 1 个 WS 业务失败也被 bind 错误遮住，未到达原断言。不能把实际 114/13/1 写成 123/4/1。

因此没有确认原声明的全量通过数字；binding export 超时与 WebSocket 环境限制均明确保留。

# 总体结论

**不可送六审：阻断项 1（next_order_id 超出 wire 安全整数域）、阻断项 2（真实 World shutdown 确定性门禁缺失）。**
