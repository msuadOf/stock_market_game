# Q01 Rust Money 传输实现

## 当前契约

用户选择 `Money` 跨 JSON、WASM、Tauri、Server 与日终存档统一使用十进制整数分字符串。
Rust 内部继续是 `i64` 分，不改变价格、费用、持仓成本或交易制度。字符串只接受
`0`、无前导零的正整数及负整数；拒绝正号、负零、空白、小数、指数、非 ASCII 数字、
超出 `i64` 的值和所有 JSON number。没有新版本标记、数字兼容或迁移路径。

`AccountingAmount` 是独立的 `i128` 公司会计金额，既有传输单位为元字符串；本次没有
改动该类型，也不能通过字段名字批量将其误改为分。`FractionUnits` 同样不是 `Money`。

## 实现与证据

- `packages/engine/src/money.rs`：自定义 `Serialize` 调用 `serialize_str`，自定义
  `Deserialize` 先要求字符串，再验证规范形状与 `i64` 范围；`ts-rs` 定义改为 `string`。
- `packages/engine/tests/money.rs`：覆盖正负与零、默认玩家资金 `1000000000000` 分、
  超 JavaScript 安全整数的 `9007199254740993` 分、`i64::MIN/MAX` 精确往返，及非法输入拒绝。
- `Position` 与 `PositionSnap` 的 `invested_cents/recovered_cents` 保留内部 `i64`，通过
  `money::cents_decimal` 复用中央 `Money` serde；`BeliefDebugSummary` 的每股估值分字段同样
  精确输出字符串，不借由 JavaScript number。测试使用极值仅验证序列化，不宣称该持仓
  是可恢复的合法市场状态。
- 红证据 `.tmp/q01-money/red-test.log`：25 个 case 中 4 个失败，直接证明旧数字形状、
  接受 JSON number 与 TypeScript `number` 不符合新契约。
- 绿证据 `.tmp/q01-money/green-test.log`：25 个 case 全通过，执行约 0.00 秒。
- 裸字段后续红证据 `.tmp/q01-money/position-red-test.log`：新增 2 个 case 失败；绿证据
  `.tmp/q01-money/position-green-test.log`：28 个 case 全通过，约 0.44 秒，包含实际
  `ts-rs` 导出 `Money.ts` 与 `PositionSnap.ts`，不是手写生成结果。
- 编译使用 `cargo test -p engine --test money --no-run -j32`，外部 300 秒上限；测试二进制
  使用外部 10 秒上限与 `--test-threads=32`，未运行完整回归。

## 跨层后续门禁

WASM 的 `to_js`、`public_dto_to_js` 与 `save_to_js` 均直接使用 serde serializer，
`serialize_str` 不会经过浮点整数转换。Server 与 Tauri JSON 同样消费中央 serde 实现。
扫描没有发现字段以 `serde(with)` 覆盖 `Money` 传输。

其余 TypeScript 文件仍须由实际 `ts-rs` 导出重新生成，不能手写生成结果。所有当前存档 fixture、
JSON 命令 fixture 和按旧数字读取 `Money` 的测试需要按真实字段类型更新；不能改动
`AccountingAmount`、ID、股数、日期、比例与其他数值类型，也不能改写历史见证原始字节。
本模块短测不能替代跨宿主验证与非作者完整 diff 复核。

`diagnostics.rs` 的 `trade_event_turnover_cents`、`total_daily_turnover_cents` 与
`DailyStat.turnover_cents` 为独立 `u64` 累计成交额，不能直接收缩成 `Money` 的 `i64`；
这些精度边界由总协调者另行核对，本模块没有改变其契约。

因果诊断的 `Quote.bid_cents/ask_cents`、`CausalFactKind` 中成交额、价格、预算及预算列表、
`OrderLifecycle.filled_value` 和 `CausalCollector.filled_values` 已复用中央 `Money`
字符串输出。可选值保留 `null`，集合保持原形状、金额元素为字符串。没有改动诊断内部
计算、指数、收益率、成交股数或 `lifetime_civil_seconds` 时间单位。

`.tmp/q01-money/causal-red-test.log` 记录新增跨金额面 case 的红证据；
`.tmp/q01-money/causal-wire-green-test.log` 中该 case 独立通过（约 0.20 秒）。诊断 feature
的 28 个 Money case 也通过。但 `causal_diagnostics` 全部 13 个短 case 中只有 11 个通过，
2 个失败：`npc_execution_reconciles_every_share_and_preserves_provenance` 没有 impact，
`real_fill_stream_rejects_duplicate_fill_and_wrong_execution_price` 没有可供破坏的 Filled。
独立重跑仍失败，日志分别为 `causal-npc-repeat.log` 与 `causal-fill-repeat.log`。
随后从 `git show 675ac4c:packages/engine/tests/{causal_diagnostics,diagnostic_parity}.rs`
提取原测试，使用独立旧源码编译的 `baseline-lib/libengine.rlib`（位于 golden 任务临时目录）
进行真实复现。Rust 版本为 1.96.1，旧 engine 启用 `simulation-diagnostics` 和
`verification-harness`，测试 `rustc --test` 使用 `-Ccodegen-units=32 -Cdebuginfo=0`，
链接同一 target 的 serde_json 依赖，没有混用当前 engine。两个旧测试进程并发，分别
使用外部 10 秒上限与 `--test-threads=32`。

旧源码同样失败，原行号 207 的 impact 断言与原行号 308 的 Filled `unwrap` 分别耗时
2.24 秒、2.25 秒，退出码均为 101。日志在 `.tmp/q01-money/baseline-tests/` 的
`build.log`、`npc.log` 与 `fill.log`。因此这两项失败已确认存在于 Q01 前的 `675ac4c`，
并非本次字符串序列化引入；本批没有弱化其断言或顺带修改其 fixture。

新增 `collector_filled_values_serialize_as_exact_decimal_cents` unit case 需在协调后的
诊断 feature lib 编译与短测中执行；总协调者已在最新诊断 feature 二进制上短测通过。

独立复核发现 `HoldingEpoch.institutional_fees_paid` 虽然是 `Option<Money>`，但仍有旧
TypeScript `number | null` override。新增 `institutional_fee_money_has_string_types_and_nonnull_exact_roundtrip`
测试实际取得类型红证据（`.tmp/q01-money/holding-red-test.log`），随后改为 `string | null`，
并以 `i64::MAX` 非空费用验证精确往返与旧 JSON number 拒绝。`Money` 模块头部已引用
ADR-0031。所有 Money 相关 TypeScript override 搜索中没有发现其他同类漏项；盘口
`Array<[Money, number]>` 中第二元素仍是股数，不应改为金额字符串。

最终 `--all-features` 编译使用 `-j32` 与外部 300 秒上限；Money 短测 29/29 通过
（约 0.27 秒），实际导出包含 `HoldingEpoch.ts`。collector 映射 unit case 独立通过
（约 0.00 秒），日志为 `.tmp/q01-money/holding-green-test.log` 和
`.tmp/q01-money/collector-green-test.log`，各二进制均使用外部 10 秒上限与
`--test-threads=32`，没有运行完整回归。
