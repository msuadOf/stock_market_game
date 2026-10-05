# 累计成交额宽度

## 领域边界

累计成交额不是单账户 `Money`：多笔合法 `i64` 分成交可以累计超过 `u64`。日 K、分钟事实、分时均价分子与诊断对账应统一使用 `u128`，JSON 使用规范非负十进制分字符串。单笔价格、账户余额、费用、申报数量与成交量的既有类型和交易规则不变；不引入格式版本、数字金额兼容或迁移。

## TDD 证据与当前状态

- Rust 新用例 `session::candles::candle_book_tests::four_legal_trades_preserve_turnover_above_u64` 经真实 `SessionCandleBook` 累计四笔各 `5_000_000_000_000_000_000` 分、各一股成交，要求量四股、笔数四、总额 `20000000000000000000` 分及归档一致。root 统一 host39 编译后，实际 exact 执行1项失败，原因是旧实现第四笔累计额溢出；日志 `.tmp/retained-turnover/cumulative-red.log`。
- Rust 新用例 `daily_turnover_wire_requires_canonical_decimal_strings` 验证拒绝数字、空字符串、前导零、正负号、空白、小数与 `u128` 越界。旧 `u64_decimal` 对部分非规范输入过于宽松，host39 exact 实际执行1项失败，接受了 `"00"`；日志 `.tmp/retained-turnover/wire-red.log`。旧红阶段源码保留于 `.tmp/retained-turnover/candles-red.rs`。两项并行、Rayon8，各exact外9000ms，共享整命令10000ms deadline。
- Web 首红已实际运行：旧日 K Wire parser 把 `20000000000000000000` 拒为超 `u64`；不是测试工具错误。新增 `utils/turnover.ts` 共享规范 `u128` parser，日 K Wire、存档、VWAP 输入输出和分时图均复用。新范围／规范测试与相邻分时测试合计16项短测通过，整命令使用外部10000ms deadline，Node case timeout10000ms、并发8。

## 尚待闭合

Rust 根修已写：共享 `orderbook::canonical_u128_decimal` 供日 K、分钟及 VWAP 使用，三个宿主 adapter 统一调用 `parse_turnover_cents`，诊断累计额同步扩容；日 K reducer 只成功安装局部 candidate，统计先完成全部 checked 计算才赋值。root 统一 host41 完整编译成功后，从实际 manifest 确认 `engine-48220fcc6e1a5073` 并 `--list` 验证名称；四笔合法大额、规范 Wire、溢出原子性、最大值 roundtrip、宽额 VWAP 与诊断计数共六项 exact 均实际执行1项通过。日志 `.tmp/retained-turnover/host41-*.log`；整批10000ms、各case9000ms进程树 deadline，8进程并发、Rayon4。

相邻 `candle_counter_overflow_returns_fatal_without_committing_the_tick` 在 host41 实际失败：其 fixture 于2030-01-01元旦休市入队，尚未抵达溢出注入；已把该 fixture 的开局明确设为2030-01-02，保留全部 typed fatal、outbox 与状态回滚断言。root 随后统一 host42 编译成功，实际 `engine-de08d335cdbb058d` 经 `--list` 确认全部名称，六新增及该相邻用例七项 exact 各实际执行1项通过，最后一项1.15秒。证据 `.tmp/retained-turnover/host42-*.log`，仍为整批10000ms、各case9000ms进程树 deadline、8进程并发、Rayon4。

非作者复核已发现并修复旧 Web 累计额负例（从 `u64` 越界更新为 `u128` 越界，证券股数负例仍保持 `u64`），ADR0031 对应段同步说明范围。Web protocol 与 VWAP 共17项短测通过，`tsc -b apps/web/tsconfig.json` 在外10000ms监督下 exit0。非作者亲读 host42 七项实际绿日志并复核日期修正增量，当前源码与定向短测门禁限定通过，结论见 [`turnover-width-review.md`](turnover-width-review.md)。上述短测不证明完整 Actor 大额成交矩阵，也不替代真实 WASM rebuild 或完整回归。
