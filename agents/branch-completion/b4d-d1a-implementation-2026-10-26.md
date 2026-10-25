# B4d D1a 实施与验证记录（2026-10-26）

状态：最终源码与运行证据已冻结，25 项 D1a 及 43 项相邻回归通过；等待未实施者完整 diff 终审，不代表 D1b 或旧 B4d 四阻断关闭。

## 基线、范围与接口

- 独占树 `.worktrees/sol61-b4d-d1a`，分支 `feat/b4d-admin-transaction-d1a`，基线 `b172c1ab1a69daa44dc57dddc887db255d7f6ce7`。
- 按 R3 冻结设计 SHA-256 `4fc6728c298e201c6fd077bec5e7ed5f825127927e9be8f021877082c85e6d74` 实施；独立设计 reviewer 为 `sol61_review_d1a_design`。
- 只新增 runtime 行政数值事务：专用 AdminOperation 四变体、World checked 数值 id、同步 with_admin_transaction、三阶段精确回执、组端 touched 完整 Arc 前值、明确终止及 panic 运输。
- 旧 AdminRight／DayEnd 路径保留；没有修改 drive／failure installer、生产公司链、#122 或导入旧 4000 行账簿。
- 设计 reviewer 已确认专用枚举与同步 API；另解释 commit 可将组内 committed_tick 对齐已发布 cur_tick 作为旧 undo 退役元数据。Prepare／Abort 仍不改游标；不推进 World.global、PublishedTick、T+1 或计划交易日。该问答已追加在独立设计复核原件。

## TDD 原件与实际结果

所有原件统一保留在主仓 `agents/branch-completion/d1a-logs/`。预构建使用独占根 `.tmp/sol61-b4d-d1a-target`，`cargo test -p engine --lib --no-run -j 16`，外置 `timeout --signal=TERM --kill-after=1s 299s`。普通运行使用预构建二进制，外置 9 秒 TERM + 1 秒 KILL，内部 `--test-threads=16`，整个批次受 10 秒进程树上限约束。

| 原件 | 实际结果 |
|---|---|
| `red-build.log` | 失败：fixture 的 Validator 闭包参数笔误；这是无关编译失败，不算行为红证。 |
| `red-build-fixed.log` | stub 可运行二进制构建成功。 |
| `red-behavior.log` | 11 个 fixture，7 红／4 绿，0.01 秒；stub 尚无落账／跨拍／panic 运输。只拒绝的 4 项此时不足以单独证明实现。 |
| `implementation-build.log`、`green-initial.log` | 初实现 11／11 绿，0.01 秒。 |
| `expanded-red-build.log` | 失败：新增 fixture 缺 AdminPhase import；不算行为红证。 |
| `expanded-red-build-fixed.log`、`expanded-red-behavior.log` | 17 个 fixture，16 绿／1 红，0.93 秒；部分发送注入尚未消费，成功返回被测试抓住。 |
| `id-red-build.log`、`id-red-behavior.log` | 包含旧 AdminRight 正控共 20 项，19 绿／1 红，0.93 秒；pending 期间被拒绝的新 id 随后被重用。已修复最后已见 id 的单调消费。 |
| `final-build-r1.log`、`final-tests-r1.log` | 最终候选首轮构建 272 秒成功；D1a 22 项 21 绿／1 红，0.93 秒。新增 NPC fixture 误给两组共四账户仅两个全局 profiles，第二组没有 NPC；已修为四 profiles，保留每组决策次数断言并新增每组 attention 长度断言，待重建运行。 |

独立预审新增 I1—I3 后，`review-red-build.log` 构建 181 秒成功，`review-red-behavior.log` 三项真实 0／3 红：sever 后行政入口未判死 join、prepare 致命失败后旧 DayEnd 窗口未关闭、阶段收齐忽略已排队额外回执。修复后所有 terminal 统一关旧窗并完整回收，纯业务拒绝仍保持旧 DayEnd 合法撤销；每阶段精确收齐后仅非阻塞检查已入队额外消息，不增加第四阶段或声称检测未来任意迟到消息。

I4 补强 panic 负控：复用真实 100 股成交的非空旧 undo，协调者／prepare panic 比较完整行政前账户；commit panic 比较所有市场股份／冻结／成本及未提交组完整账户，保原 Arc payload 身份。`npc-fixture-fixed.log` 已单独验证修正全局 profiles 后 NPC 正控 1／1 绿。

最终 `final-build-r2.log` 暖构建 34.94 秒成功；`final-tests-r2.log` 与带源码前后指纹的 `final-tests-bound.log` 均 25／25 绿，约 0.93 秒；相邻 `day-end-regression.log` 8／8、`day-end-public-regression.log` 29／29、`seal-regression.log` 6／6 全绿。普通命令均在 10 秒外置进程树上限内，内部并发 16。

[`d1a-logs/final-manifest.json`](d1a-logs/final-manifest.json) 绑定真实最终源码前后 hash、二进制 SHA-256 `ba7ab91a761ccf07dcba4b1207df069e90ecc64fd29f93e3396431e643606888`、构建日志、执行命令、退出码、并发及用例数。源码与二进制 mtime 一并保留，源码运行前后无变化。部分进程采样在构建完成后才取得，表内无 PID，不把它冒充构建运行中的 CPU 证据；运行期工具确曾观察 rustc 的 CPU／线程，未做性能结论。

以上红证是分步真实运行；没有声称每个新增 case 都曾独立红，也不以 API 缺失或 import 笔误替代 13 类行为验收。新增短测均使用独立 World，无共享可变业务全局。

## 13 类合同映射

测试位于 `runtime/world/admin_transaction.rs::tests`；组内日志／id 细节另在 `runtime/group/admin_transaction.rs::tests`。

| 冻结组号 | fixture 后缀及实际断言 |
|---|---|
| 1 | `admin_01_two_groups_all_operations_persist_through_real_tick`：双组、尾组 local=1、四操作及完整账户比较；`admin_01_normal_npc_decisions_continue_after_credit`：真实 NPC 决策、挂单与现金＋费用守恒正控。 |
| 2 | `admin_02_rejection_restores_complete_accounts_and_new_id_retries`：完整账户恢复、新 id 仅入账一次；组内同号 fixture 比较 undo／Seal／plan_book／RNG、attention、游标与完整账户。 |
| 3 | `admin_03_other_group_rejects_and_original_order_is_not_netted`：另一组不足／local 越界／stock 越界，所有账户不变、原错误明确，先扣后贷不得净额掩盖不足。 |
| 4 | 两个 `admin_04_*`：大于 i32::MAX 的增减与 Abort、raw 上溢、cash＋held／shares＋pending 上溢及恰好可表示正控，零／负值拒绝。 |
| 5 | 两个 `admin_05_*`：frozen／pending／成本全字段恢复、可卖不足、真零成本与 ever_held、行政再贷记、仍有真实在途买票拒绝、归零后真实买入从新成本累计。 |
| 6 | `admin_06_rejection_preserves_published_tick_and_next_step`：真实成交、已发布 Arc 句柄／市场委托／全部账户保持，再真实 tick 正控。 |
| 7 | `admin_07_rejection_keeps_real_day_end_abort_legal`：成功日结后行政拒绝，再真调用 AbortDayEnd 恢复完整账户及冻结。 |
| 8 | 两个 `admin_08_*`：commit 关闭旧 DayEnd 窗口、下一 tick 业务拒绝保行政基线，真实 Sender 断连的 partial Start 判死／join 且保已发布 tick。 |
| 9 | `admin_09_closed_days_without_start_keep_first_commit`：无 Start 的第一笔成功／第二笔失败保第一笔，再真实 step 持久。 |
| 10 | `admin_10_partial_prepare_and_commit_send_failure_kill_and_join`：真实断连 Sender::send，准备／提交部分发送，候选不发布、永久拒绝与句柄全回收。 |
| 11 | `admin_11_real_channel_receipt_id_group_locals_and_missing_are_fatal`：三阶段真实通道错误 id／group／locals、缺组、重复不得填缺组；组内 id 合同覆盖过期／重复／pending 新 id 消费；pending 期真实 Start／Continue／DayEnd 均明确拒绝且不倒放市场账户。 |
| 12 | `admin_12_seal_commit_drain_abort_restores_orders_and_resources`：真实挂单、Seal、行政 commit、同 tick DayEnd／Drain／真实 Abort；完整账户、在途资源和完整 RestingFact／游标全等，随后真实 tick。 |
| 13 | 两个 `admin_13_*`：两组准备后协调者真实 panic，或 worker Prepare／Commit 真实 panic；外层核原 Arc payload 身份、全部句柄已 join、World 永久拒绝、已发布句柄与 tick 不退。 |

另有空事务只调用一次闭包、重复组／越界组、id 耗尽的边界；空事务无虚假 worker 消息。三项 `admin_review_*` 对应上述独立预审发现的真实先红后绿。

D0 契约已只读核对独立树的 `RuntimeDriver::install_account_projection`／`install_day_end_accounts`：其读取完整 GroupAccount，以 cash＋held／shares＋pending 恢复公开资源并处理真零仓。新 AdminGroupCandidate 使用同一完整 GroupAccount，操作后 checked 聚合边界使其不产生不可表示的公开资源；本批没有触发 installer，生产当日安装与 save／restore 仍由 D1b 接线验收。

## 领域边界

沿冻结稿与既有官方取证，不新定交易制度／税率／日程；ShareSplit 仅承载已经可流通送转到账，ShareReduction 是可卖在手股份数值核减原语。长期限售、lock_until、冻结股整体缩股、混合来源、碎股和任意缩股方案不支持，已在 `docs/trading-rules.md` 正式登记。没有本日重新联网取证，不冒充已完成全量现行规则重验。

## 待办门禁

- 最终预构建、25 项短测与 43 项相邻回归、源码／二进制 hash 绑定均已完成。
- 未参与实现的 Sol 对完整 diff 做 A 股语义／最小范围／边界与复杂度审查；有效发现修复后再次复核。
- 通过后创建聚焦本地提交；不合 main、不 push，本地 main 集成由 Leader 另行协调。
