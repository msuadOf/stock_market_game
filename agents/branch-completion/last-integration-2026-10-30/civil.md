# civil 集成失败归因与修复记录（2026-10-30）

本片只修改 `packages/engine/tests/runtime_semantics_anchor.rs`、
`packages/engine/tests/runtime_flip_batch4.rs` 与本记录，不修改生产实现或交易制度。
独占 worktree 为 `.worktree/last-integration38-civil-2026-10-30`，分支为
`codex/last-integration38-civil-20261030`，基线为
`7057a495435d9edd9f1032d94f365d22dab79ffe`。

## 原始失败与分类

真实失败保留在主工作区
`agents/branch-completion/validation-scheduler-2026-10-29/integration-unreached-e1af5aaa-2026-10-30/failed-cases.json`：

- `civil_day_end_failure_rolls_back_and_retries_once`：572 ms，断言会话 day 回滚时实际为 6，期望为 5。
- `batch4_civil_failure_rolls_back_day_end_fully_and_retries_once`：1835 ms，
  在 240 tick 后弃用原 driver 再重建，先报 `shared_sessions=0（应为 1）`，尚未进入 civil 失败点。

归类为旧测试契约和驱动 fixture 失配，不是本片已确认的生产 rollback 缺陷。
`RuntimeDriver::install_manual_day_end` 先安装市场镜像、确认来源并交付市场事件，再调用
`session.end_civil_day()`；此后普通 civil 不变量失败封存首个 fatal，不能撤回已发布市场。
原失步注入 `rollback_runtime_day_end(5, seq)` 会先安装市场 day 为 6，再被真实
`CivilClockError::MarketSessionOutOfSync { completed_sessions: 6, expected_sessions: 1 }` 拒绝。

取代旧合同的提交为 `1d50cb2495223cfeaad30c8e913f8677cbd86b3d`
（2026-10-27，`feat(engine): 汇合日结Full候选与非tick来源门禁`）：
该提交删除旧手动日结的失败 rollback 编排，引入先安装市场镜像、后推进 civil 并 poison 的
`install_manual_day_end`。本片基线的源码锚为：

- `packages/engine/src/runtime/drive/convergence.rs:667`：手动市场日结入口；
- 同文件 `:766`、`:767`：市场事件交付先于 civil 推进；
- `packages/engine/src/runtime/drive_tests.rs:4631`：既有
  `d0_civil_failure_preserves_installed_day_end_and_first_error` 直接覆盖该合同；
- `packages/engine/src/session.rs:3770`：真实计数失步拒绝。

`docs/trading-rules.md` 的“个人信息 anchor 与普通显式投递边界”已登记：实际市场收盘后的
普通 civil 失败保留收盘并终结会话；明确可取消后重试的真实更正 prepare 业务拒绝是受限例外。
本片沿用该已登记边界，不把计数不变量错误伪装成可恢复业务拒绝。

## 最小修复与断言

两个测试保留原计数失步注入，诚实改名为：

- `civil_day_end_failure_preserves_installed_market_close_and_first_error`；
- `batch4_civil_failure_preserves_installed_market_close_and_first_error`。

flip 测试删除概率搜索、弃用 driver 和额外重放 240 tick，改为同一 driver 跑首日完整 240 tick。
末拍时显式验证 `(tick, day) == (240, 0)`，确认市场拍完成不等于显式市场日结已安装。

两例均精确验证真实 civil cause 与首个 fatal；检查已安装市场 day、原 civil 日期、恰一日
retained 和真实收盘竞价事件；逐事件核对连续 seq，并核对 Session 与 driver 的最终 seq 相等。
随后保存、civil 日结、继续市场 step、重复手动日结均必须返回同一个首错；这些拒绝不得再改
business hash、tick、day、seq、publication 或再次产生清簿事件。

此处 day 为 6 是原错误注入后的已提交市场状态证据，不能单独当作通过条件；测试同时约束
真实错误位置、原 civil 日期和全部后续终态行为。原“恢复 day 后可续跑”的断言撤回并改为
明确拒绝续跑，不宣称这两例仍证明市场提交可回滚或 civil 可重试。

大 A 语义依据沿用 `docs/trading-rules.md` 已记录的市场交易日、收盘与自然日分离约定及
官方交易日规则；本片不新查或改写交易所制度，不以本片测试声称恢复完整市场日结能力。
金额仍为分、持仓仍为股、T+1、撮合与费用均未改变。

## 验证与门禁状态

- `rustfmt --edition 2021 --config skip_children=true`：两个修改的 Rust 文件通过。
- `git diff --check`：通过。
- 本片未启动 Cargo、metadata、target 检索或测试 binary；唯一验证调度为 `/root/sol_integration_29`。
- 调度要求先完成独立复核与净源接入，再在唯一 dev committed clean HEAD 中统一构建 7 个 suite，
  按新精确名称运行。本片尚无新的运行 green，不将源码检查或原失败日志冒充修复后通过。
- 未实施本片的 `/root/sol_last_integration38` 负责独立复核完整 diff；复核和集中验证结果由父任务
  记录后方可声明完成。

Rust 冻结内容 SHA-256：

| 文件 | SHA-256 |
|---|---|
| `runtime_semantics_anchor.rs` | `a2f1c75b61be259e6f78041d6fc0dfb96e38427958dbbe2042c52230ba3ff6d7` |
| `runtime_flip_batch4.rs` | `c98e4086ca210ebedadc82869d4d579f2997329c9f06f024693d521dad660cd0` |
