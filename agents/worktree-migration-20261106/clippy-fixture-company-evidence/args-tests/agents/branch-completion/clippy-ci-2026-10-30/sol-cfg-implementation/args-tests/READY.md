# Clippy test helper args 收窄完成记录

日期：2026-10-30
分支：`codex/clippy-cfg-args-tests-20261030`
基线：`50d493701aa0fbb7d62563855a84dc07c89c29b0`

## 改动范围

- `runtime/group.rs`：为仅测试使用的 `GroupState::new` 加 `cfg(test)`，改收命名字段的 `GroupSetup`。该结构表达组索引、账户、撮合批次与整手、配置、日拍数、决策初始态、计划与脚本等实际初始化事实；委托 `new_with_initial_tick` 的值及顺序保持不变。
- 将上述构造入口的 14 处调用迁移到 `GroupSetup`，涉及 `runtime/group.rs`、`runtime/group/admin_transaction.rs` 和四个测试子模块。全仓检索未发现 apps、examples 或 integration tests 消费；`world.rs` 生产路径使用 `new_with_initial_tick`。
- `session/day_boundary_kline_tests.rs`：把 `assert_day_k` 的 8 个期望值改为 `ExpectedDayK` 命名字段，原 7 处调用全部迁移；原逐项成交量、成交额、笔数、OHLC 断言及文案保持。

## 语义与验证

仅改测试构造和测试断言的参数形态。`turnover_cents` 仍为分，成交量仍为股，成交笔数与价格分字段逐项核对；没有改变撮合、结算或生产路径。

- `git diff --check`：通过。
- Cargo、target、Clippy 与测试命令：按任务要求未运行。
- 独立 diff 复核：待主 agent 完成；本记录不宣称已通过。

## 冻结信息

代码 diff（8 个 Rust 文件，不含本记录）的 SHA-256：
`a13e09485cdc6dbd2df94a88055a71e2468d13eaccd7cf8d46e6bd24e699e09f`
