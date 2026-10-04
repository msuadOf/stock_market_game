# Q01 受控原字节表示取证

本记录只审计 `Money` 整数分传输表示，不能把失败断言的 `left` 当作新预期，
不能借表示重钉掩盖撮合、现金、成本、股份、费用、RNG 或事件顺序的业务变化。
没有存档版本、历史格式兼容或产品迁移入口。

## 独立生产者

`money-wire-golden.rs` 完整复制 `extraction_replay` 的 setup、库存再分配、真实
Player 卖单、自然 NPC 买单、自然日日结与生产调用，并保留库存不注资、真实
成交、Player/NPC 对手方、买入当日 T+1 lock、费用减少现金、日界、集合竞价、
日期及股份守恒 guards。`step_skeleton` 使用原完整日 fixture，保留价格笼子开关、
无挂单和无成交断言。工具不执行固定摘要断言，也不读取断言错误输出。
两个原测试的固定摘要、同 seed 比较、完整 restore 与 seed/order 扰动断言不删除。

旧原字节来自 `git archive 675ac4c packages/engine/src` 的实际源码，独立编译后
链接同一 producer；不是把新输出去引号伪造成旧见证。两个测试 fixture 相对
`675ac4c` 无变化。旧 archive、rlib、实际 producer、日志与完整捕获留在
本主题 `.tmp/money-wire-golden/`，历史原始 capture 不修改。

编译为独立长验证，`rustc 1.96.1`、`codegen-units=32`、外部 `300s` deadline，
旧 engine 使用 `simulation-diagnostics` 与 `verification-harness`，独立 out-dir，
不争用 Cargo 锁、不运行生成类型导出。五个 capture 进程并发，分别为
auction `0/3/6`、固定 seed 与扰动 seed，每进程外部 `10s` deadline，全部 exit 0。
单 worker 仅约束每个受控 fixture 的生产受理顺序，不要求自由并发调度逐字节相同。

## 基线真实捕获

FNV-1a 64 按原字节计算；step 按 tick 顺序连接 20 个完整 projection。
`675ac4c` 实际运行独立复现全部六个固定锚，且 replay 原字节 SHA-256 与此前
`merge-characterization.md` 记载的真实见证一致。

| 场景 | 基线 FNV-1a 64 | 基线 SHA-256 |
|---|---:|---|
| step auction 0 | 18099143602724613260 | `d81ebca6c98d11ed09969a08672250c73f70a21b83c8219f544e5384bd9d46ca` |
| step auction 3 | 16192716476036528973 | `52dd59de9944b40c0011219463e38b6d2b77572ab9d4af856a7daff13e944836` |
| step auction 6 | 2620780488067564097 | `63fefa8ef715cf12cef9ee7248f33086261a51e1e95e1c21ac64c69ad3e3ea06` |
| replay events | 7922886018261573110 | `3e70a76092ccd83d0cdf245fe6bb14a122c4e43d05b8628bb18e9b2bbae05e99` |
| replay mid | 9809656468401244634 | `501914c5ce3cdb8e5ecbaad7a2c4bbb653733dfd5412ac8b29b08e3c760a3ff9` |
| replay end | 15011441865679768707 | `41936b82db6f252e258bf424c9c3758219fe689d87e4108ed8f70a968040bc80` |

## 当前表示与逐 path 对照

当前 producer 链接实际 Cargo all-feature artifact
`target/debug/deps/libengine-76f3c9cc662c8afd.rlib`；其来源由
`.tmp/q01-money/all-feature-build.jsonl` 的 `compiler-artifact` 明确记录，
features 为 `simulation-diagnostics` 与 `verification-harness`，不是根据文件时间猜测。
依赖为同构建的 `librayon-f4e12f99532c978a.rlib` 与
`libserde_json-64d03d242a680cbe.rlib`。两边使用相同 producer 原文，五个当前捕获
进程同样全部 exit 0、业务 guards 全部通过。后续 `HoldingEpoch` 的 `ts` annotation
修正只影响类型导出，不改变此原字节契约；实际 Cargo 短测试由总协调者另行复核。

`money-wire-golden-verify.mjs` 对新旧原文逐 token、逐 path 核验，数字 lexeme
不经过浮点解析。仅允许已核对实际类型的分字段数字被精确加引号；其他所有
原文（键、字符串、金额之外的数值、标点、数组顺序与空白）必须完全相同。
许可 path 依据是 `GameConfig`、`StockSpec`、`NpcSetup`、`Event`、`DailyCandle`、
`Snapshot`、`AccountSnap`、`PositionSnap`、`MarketMinuteClose`、`RetailExperienceState`、
`HoldingEpoch`、`OwnObservation` 和 `StockPriceMemory` 的真实 Rust 字段。
盘口 tuple 仅允许第 0 项 `Money`，绝不将第 1 项股数当作金额；setup 的 `tick`
是每股最小报价单位，其他 tick 时钟数值不获许可。`AccountingAmount` 公司会计元
字符串、所有已有 `u64` 字符串、比例与 ID 没有任何许可替换路径。

66 份真实 capture（60 个 step projection、两个 seed 各 events/mid/end）全部满足
“旧原字节仅在许可分 path 精确加引号后等于新原字节”。没有业务事实、事件顺序、
现金、成本、股份、费用或 RNG 变化。新锚来自独立当前原字节 FNV/SHA，不来自测试失败。

| 场景 | 当前 FNV-1a 64 | 当前 SHA-256 | 精确加引号 token 数 |
|---|---:|---|---:|
| step auction 0 | 14588880118627202930 | `ad457f8335dfa268d6dfe6448f65e199d0d67b43f5fb520830ad3931c24bc68c` | 60815 |
| step auction 3 | 10855500538460332623 | `0178626184573d8c48c64815e0584d30ca75c111d32e5861c50ca7a67085e7db` | 60381 |
| step auction 6 | 14140194587992130069 | `37945a08cde13dfec5639707b89aab3c7a0f6084037fb30591337864b08a663e` | 59942 |
| replay events | 9977927079659249770 | `9d13241d831879ec2898ece70c99c705f1f2537aa9fed6ccdd43b242ed917185` | 8299 |
| replay mid | 9397357771902842944 | `594659291d2d68adf535a9694e4983352979977209993dcb54edac5d63072687` | 3224 |
| replay end | 10556944033955451089 | `83b8bf6ca8e0ad6246f4375f7375c135240b49006cc8d65241951f4ac2d8bf6c` | 3251 |

扰动 seed 的当前 events/mid/end FNV 分别为 `2499139833862702270` /
`9091156116303912163` / `14775378269680792483`，同样严格通过原字节对照。

## 短验证及复核

原文核验最初使用逐字节 BigInt FNV，独立复核在其他编译任务并行时触发 step
的 9 秒外部 deadline，该次不能计为通过。改为精确的高低 32 位 FNV 运算后，
进一步将每个 step 的原文核验拆成四个独立五 tick 分片，完整 20 tick 摘要单独
计算；没有缩短真实 fixture 或放宽测试限时。全部旧锚与新 SHA 再次核对一致。
最终 15 个 step 核验进程（12 个原文分片 + 3 个完整摘要）并发、外部 10 秒限制，
全部 exit 0，最长 4.95 秒；两个 replay 核验进程分别 0.87/0.73 秒。

复算入口（`BASELINE`、`CURRENT` 为本主题 `.tmp/money-wire-golden/` 两 capture 目录）：

```sh
node agents/remaining-questions-and-features/money-wire-golden-verify.mjs "$BASELINE" "$CURRENT" 0 0
node agents/remaining-questions-and-features/money-wire-golden-verify.mjs "$BASELINE" "$CURRENT" 0 1
node agents/remaining-questions-and-features/money-wire-golden-verify.mjs "$BASELINE" "$CURRENT" 0 2
node agents/remaining-questions-and-features/money-wire-golden-verify.mjs "$BASELINE" "$CURRENT" 0 3
node agents/remaining-questions-and-features/money-wire-golden-verify.mjs "$BASELINE" "$CURRENT" 0 digest
node agents/remaining-questions-and-features/money-wire-golden-verify.mjs "$BASELINE" "$CURRENT" replay
node agents/remaining-questions-and-features/money-wire-golden-verify.mjs "$BASELINE" "$CURRENT" perturbed
```

auction 3/6 同样分别运行 0–3 分片与完整 digest，每条命令应由外部 deadline 包装。
最终两个原测试由 `rustc --test` 链接同一实际 all-feature rlib，多核并行编译；
9 个 case 各独立进程并发、外部 10 秒限制，全部通过，最长 case 5.33 秒。
固定锚、同 seed 原字节、完整 restore、seed/order 扰动及原业务 guards 全部保留。
`rustfmt --check` 与限定 diff whitespace 检查通过。未执行完整回归。
总协调者随后从最终 Cargo 产物另行执行 6 个 golden case，全部通过、最长 5.20 秒，
日志位于 `.tmp/q01-money/final-<case>.log`；最终类型 annotation 修正未改变这些锚。

非作者 `q01_review_fixtures` 全文审查两测试 diff、producer、核验工具和真实 capture，
独立复算最终 17 个分片／摘要进程，外部 9 秒限制下全部 exit 0、整批 wall 5.14 秒；
旧新 FNV/SHA、66 份原字节与实际字段类型均通过，未发现语义或必要性阻断。
