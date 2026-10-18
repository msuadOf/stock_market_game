# 二次核对：语义追溯矩阵刷新（Haiku 实习生产物复核）

- 复核对象：`.tmp/haiku-traceability-refresh.md`（2026-10-18）
- 方式：只读 `rg` 独立验证（未运行 Cargo，与实习生同等限制）
- 路径别名与实习生报告一致（desk/factory/scope/skeleton/drive/anchor）

## 一、抽样 6 行正向验证（测试存在 + 断言与规则面相关）

| # | 规则面 | 声称锚点 | 独立验证结果 |
|---|---|---|---|
| 1 | R21 | `r21_capacity_convention_no_fixed_count_rejection`:762 desk | 命中。断言 300 张买单全额受理（现金预留手算 oracle 27_150_300）、挂簿深度 30_000、`skipped_orders=0`、Drain 全额回流——直接锚定"不按固定条数拒收"。 |
| 2 | R22b | `r22b_classification_cadence_matches_registered_policy`:237、`r22b_belief_horizon_days_by_classification`:289 factory | 命中。逐风格断言 `daily_plan_review` 旗标、观察概率落在手算区间并对照 1 次/日判别线；horizon 60/20/5 逐风格断言。 |
| 3 | R01b | `r07_continuous_self_cross_terminates_without_self_trade`:1234 desk | 命中。测试名带 r07 前缀但 doc 注释明确标注"R01b/#7"；断言 `desk_trades` 为空、双边挂单原位保留（best_ask/best_bid 均 1000）——锚定自单交叉终止不自成交。 |
| 4 | R08 | `r08_cancel_window_partition_by_receipt_and_clearing_participation`:998 desk | 命中。窗内撤单→`Cancelled` 事实余量 100、无 `CancelWindowClosed` 回执、撤他人单→`OrderNotFound`——与备注描述一致。 |
| 5 | R09 | `r09_partial_fill_cancel_releases_remainder_only`:1106、`r09_fully_filled_cancel_returns_already_filled_receipt`:1165 desk | 命中。断言只撤 200 股余量、全成撤单 `AlreadyFilled`×2 / 他人单 `OrderNotFound`×1、失败撤单无 `Cancelled`、既成结算不被触碰——完全吻合。 |
| 6 | R19 | `r19_symbolic_prices_resolve_to_daily_band_edges`:886 desk | 命中。`Highest` 买臂解析=涨停 1100 且按解析价预留 purse；`Lowest` 卖臂解析=跌停 900、卖单不占现金——吻合"按解析价预留"。 |

（R16 虽不在正式抽样内，也顺带确认了 desk:798-805 的手算 oracle 注释与"保护按日涨跌幅、余量不进簿"承诺一致。）

## 二、"仍未覆盖"缺口面反向确认（抽查 2+，实际查了 4 个）

| 规则面 | 实习生判缺口 | 反向确认 | 证据 |
|---|---|---|---|
| R24 | 计划根逐个完成/terminal 生命周期未完整锚定 | **属实** | drive_tests.rs:1464-1472 测试内注释自认："原测试名承诺含 terminal 但 fixture 在 belief 链下无法产生 terminal 计划记录"——该锚确为空承诺。`rg "plan.*root|根计划|逐个完成"` 无其他测试锚。 |
| R23 | 来源标签非优先级、同账户按接收事实裁决未直接验证 | **属实** | `rg "来源标签|source_label|origin.*priorit"` 全引擎仅 1 个无关命中（read_only_start.rs 的 hydration 优先级）；"接收事实"命中均为源码 doc 注释（npc_tick_preparation.rs:410/536/632），非测试断言。 |
| R33 | 退整、新股无涨跌幅等登记子面无测试锚 | **属实** | `rg "退整|新股|无涨跌幅"` 全引擎仅 company/opening.rs:3 一处无关注释（开局资产措辞，非交易规则锚）。 |
| R30 | 到期/暂停/存档（Web 会话面）引擎侧无覆盖、存档域无单独断言 | **属实（engine 范围）** | scope_boundary.rs:2-6 doc 明确定时自动是会话内 Web 功能，引擎仅有词表排除测试（:52）；引擎范围 `rg "定时|schedul"` 命中均为公司调度等无关域。Web 侧本次未核（实习生亦未核）。 |

## 三、判定

- 抽样 6/6 行号精确命中，断言内容与各规则面描述吻合，无虚报。
- 反向确认 4/4 缺口属实（含 R24 的测试内注释自证），无漏报。
- 净缺口维持 4 面：R30（Web 到期/暂停/存档）、R33（退整/新股无涨跌幅登记子面）、R23（来源标签非优先级+同账户接收事实裁决）、R24（计划根逐个完成/terminal 生命周期）。
- 结论：实习生报告**通过复核**，结论可直接采信。
