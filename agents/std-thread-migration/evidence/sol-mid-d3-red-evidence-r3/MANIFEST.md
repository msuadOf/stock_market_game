# 红证据清单（第三轮整改 r3 + 三审收尾 r4）

每份红日志 = 完整 cargo test 输出（含 panic 原文；数值断言含 left/right，
expect/panic 型断言为原文无值——逐份如实标注）。SHA=取证时的提交；
突变型红（r3）注明基准提交与确切突变内容（无独立提交，不可由 SHA 单独
复现——r4 起改为"测试提交先于实现提交"的可审计形态）。

## r4（三审四项收尾——红=测试提交、绿=实现提交，SHA 可审计）

| 文件 | 内容 | 命令 | 红 SHA | 绿 SHA |
|---|---|---|---|---|
| 01-red.txt / 01-red.sha | purse desk 占用对账（left=899499 right=909499——价优差额 10000 漏计） | `cargo test -p engine --lib sol4th_buy_purse` | af1d3478（仅测试） | ca96f449 |
| 01-green.txt / 01-green.sha | 同测试绿（110001 加回+Abort 恢复） | 同上 | — | ca96f449 |
| 02-red.txt / 02-red.sha | 首步前 queue staged 消费（left=0 right=1——注入缺失）。**重出说明**：30d286b3 原红混入 fixture 清槽原因（twin 后建清空 staged）且绿提交改了构造序——不可认证同源；本轮按同 fixture（twin 先行）在 b1c5abd9 上以"消费后丢弃注入"突变重出 | `ENGINE_STEP_RUNTIME=1 cargo test -p engine --lib sol4th_queue` | b1c5abd9+突变（02-red.sha 注明） | 本轮提交 |
| 02-green.txt / 02-green.sha | 同测试绿（恰一次+第二拍零新增；仅含存留的恰一次测试——原 suppresses 测试已按登记删除）。基准口径统一：c8b6c263 工作树+本轮修复（02-green.sha 注明，与 02-red 同源基准） | 同上 | — | 本轮提交 |
| 04-sametick-red.txt / .sha | 同拍结算红（禁用同拍投影+现金口径还原：买家持仓条目缺失 panic "no entry found for key"——迟一拍的原形态） | `ENGINE_STEP_RUNTIME=1 cargo test -p engine --lib sol6th_same_tick_settlement` | c8b6c263+工作树+突变（sha 注明） | 本轮提交 |
| 04-sametick-green.txt / .sha | 同拍结算绿（持仓/T+1/现金/成本/清仓+下一拍不双计；abort 不泄漏另证） | `... --lib sol6th` | — | 本轮提交 |
| 05-foreign-red/green.txt | 外来同键成交（红=组内 guard 临时回 Err 形态：权威落账拍 WorkerFailed"身份不符" ticket=OrderId(10) fill=(0,15)——Err 杀合法外来单拍的原形态实证；绿=跳过语义完整态。投影身份核的突变红在当前 fixture 不可判定——目标子单外来拍被重报价致红绿同值，如实登记） | `cargo test -p engine --lib sol7th_foreign` | 七审工作树+Err 突变（05-red 内含命令与形态注记） | 本轮提交 |
| 03bc-red/green.txt（重出） | 组内外来成交跳过语义（红=身份核移除突变——余量被吃 150 vs 200，原缺陷形态；绿=跳过守卫。原 r3 版为 expect_err 断言——Err 语义已按六审勘正废弃，红绿对随语义更新重出） | `cargo test -p engine --lib sol3_fill_with_foreign` | 七审工作树+无守卫突变 | 本轮提交 |
| 06-identity-red/green.txt/.sha | 投影身份核直接判别（Sol 八审阻断收口：R1 改为直接断言 session 投影 filled_qty——红=guard 移除突变 left=500 right=400 误推进 +100；绿=完整态。此前 R1 只断 Active+余量读组簿，判别力为零） | `cargo test -p engine --lib sol7th_foreign` | 85ce0ed7 工作树+突变 | 本轮提交 |
| 07-prune-red/green.txt/.sha | D3b 件 1 个人记忆裁剪（红=修复前 env1-final9th 内含 2081 left=9 right=8；突变复核 fixture 限制如实登记——组内 Start 裁剪先行覆盖，同拍入口保留为完成拍=末拍的一般形态） | `ENGINE_STEP_RUNTIME=1 cargo test -p engine --lib completed_sell` | a7d18a2f（红，env1 日志） | 本轮提交 |
| 09-install-red/green.txt/.sha | D3b 件 5 个人态注入通道（红=no-op 突变——watchlist 读面不可见；绿=组权威覆盖） | `cargo test -p engine --lib d3b5_install` | e83f091e 工作树+突变 | 本轮提交 |
| env1-final4th.txt | 全量 env=1（40 失败：54−14，chain (a) 适配转化） | `ENGINE_STEP_RUNTIME=1 cargo test -p engine --lib` | b1c5abd9 | — |

## r3（六项整改——突变型红，基准+突变内容）

| 文件 | 断言形态 | 命令 | 基准 SHA | 突变（=红态） |
|---|---|---|---|---|
| 02-red.txt（r3 原始版已随重出覆盖——held 红留存于本表记录） | held：left=200/right=100（突变=加回 ticket.qty，基准 a5a2a5d5）。purse 部分（left=200502/right=100001）**已被 r4 的 01-red 取代**（口径从重算下界改为 desk 占用——r3 purse 红证明的只是"旧全量加回已改"） | `cargo test -p engine --lib sol2_held_read_face` | a5a2a5d5 | held 加回 ticket.qty |
| 03a-red.txt | panic 原文（expect 型，无值）："轮 0 挂单受理事实应下行组" | `cargo test -p engine --lib sol3_continuation` | bd9c32af+未提交测试 | 未应用 world 逐轮转发修复 |
| 03bc-red.txt | 3b: panic 原文（expect_err 型 "外来单成交必须显式失败: ()"）；3c: panic 原文（assert 型 "他单的 Closed 不得释放本票"） | `cargo test -p engine --lib sol3_fill_with_foreign`、`... sol3_closed_with_foreign` | d2f67acf | 移除 fill 身份校验块+release 身份守卫块（两修复各自回退） |
| 04-red.txt | left=[(0,1)]/right=[(0,100)] | `cargo test -p engine --lib sol4_force_due` | 798455d6+未提交测试 | 未应用 ForceDue 延迟修复 |
| 04a-red.txt | left/right=完整 PlanBook Debug 串（active_child_order_id/remaining 分叉） | `cargo test -p engine --lib sol4_day_end` | 798455d6+未提交测试 | 未应用 plan_undo_saved 复位修复 |
| 05-red.txt | panic 原文（assert 型）："首步前 seed 的挂单经构造期迁移进入 runtime 世界" | `ENGINE_STEP_RUNTIME=1 cargo test -p engine --lib sol5_seed` | 192fb5bb | 注释掉 adopt_session_resting_orders 调用 |
| 05b-red.txt | panic 原文（assert 型，含种子与 dues 数值上下文）："queue 的全名册到期须在 runtime 生效" | `ENGINE_STEP_RUNTIME=1 cargo test -p engine --lib sol5_queue` | 192fb5bb | force_all_due_via_runtime 回传 u64::MAX |
| env1-baseline.txt | 全量（55 失败） | 同上 env1 | 798455d6 | — |
| env1-final.txt | 全量（54 失败——55−1 已知 flake） | 同上 | 531c6c15 | — |

如实登记：
- r3 无 01 编号（当时修复 1 未做行为红——三审判决已点名；r4 补齐为上表 01）。
- 03a/03bc/05/05b 为 expect/panic 原文型（无 left/right 数值）——判决所指
  "缺 left/right" 属实；数值型断言（02/04/04a）均有 left/right。
- terminal"实测红"（531c6c15 提交时强化断言发现 fixture 无法产生 terminal）
  原始日志未落盘（当时在终端输出中确认后回退断言）——r4 以
  sol4th_terminal_projection_overwrites_stale_active_mirror（确定性绿）
  承载该覆盖，此为空缺登记。
