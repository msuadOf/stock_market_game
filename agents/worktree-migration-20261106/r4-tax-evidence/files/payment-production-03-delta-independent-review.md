# payment-production-03 窄增量独审

日期：2026-10-29。裁决：**APPROVED_SOURCE_ADMISSION（02→03 测试增量）**。

本票承接 [payment-production-02 完整独审](payment-production-02-independent-review.md)，只核本次 case04 的真实 raw 账户回滚断言及冻结一致性，不重签接收树合成或实际运行结果。本审未实施产品；仅写本报告，未运行 Cargo、未读取 target 或计算产物大 hash。

03 的基线仍为 `779a7f913255b3e090c94d4f1a655a28c3913c14`。已核 [payment-production-03.patch](payment-production-03.patch) SHA-256 为 `6cabdb46eeb5caca7cb6175407c17a64e0cb75803defde58cd62f842d21d0881`，对应 [fingerprints](payment-production-03-fingerprints.json) 十份文件均匹配，完整 live diff 与 patch 逐字节一致，`git diff --check` 无输出。

对比02与03的冻结指纹，唯一变化文件为 `packages/engine/src/session/ordinary_tax_day_end/payment_runtime_tests.rs`，SHA 从 `893099decc4694d56759295d415af454b590634ff6e94b73d38f7540c8af62e9` 变为 `57111f32b914bdcd8494bdc10b13dd296cec0c2c87e07f4083afad9c8333765e`；其余九份文件字节未变。本审从02 patch重建该测试文件并核回其原SHA，再逐行比较03，确认净增量仅为：

- case04 在同一 driver 锁内捕获原 cache snapshot 与 `world_accounts_snapshot()`，离开块后释放锁，再进入原付款候选拒绝路径。
- 尾部 driver guard 改为 `mut`，保持原健康状态及 cache 断言，并新增真实 raw 账户快照与原值完整相等的断言。

已核 `RuntimeDriver::world_accounts_snapshot(&mut self)` 直接调用 `World::snapshot_accounts()`，返回各 group 的完整 `GroupAccount`，不是从 Session 现金或 cache 重建；前后断言覆盖完整 raw 字段。新增读取发生在事务前及 Abort 返回后，均在静点，未持有前置 guard 跨入同一 driver 的事务调用。`mut` 与真实方法签名一致。原先要求 shadow 已真实 +200 cash、`Paid` 后才主动拒绝的断言及原错误、business hash、Companies、Session cash、World 健康与 cache 对照保持不变；01..03未改。

AGENTS 三问限定：本增量不改变 A 股领域行为或产品代码；真实 raw 回滚对照用于补足02指出的测试证据范围，改动必要且最小；未弱化任何断言，未引入第二套资产算法或额外事务。未发现本增量的阻断源码问题。

case04仍为实现后的回归保护，尚未实际运行，不构成历史先红或实际 raw 回滚通过证据。02关于实际编译/green、Session既有poison策略、其他未测边界及接收方必须按 hunk 保留 causal 等新版字段的限制全部保留。本票仅准入03冻结片进入实际验证。
