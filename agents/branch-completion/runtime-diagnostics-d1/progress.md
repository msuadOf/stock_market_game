# D1 当前实施记录

## 范围与源码身份

- 独占分支：`fix/runtime-diagnostics-d1`，基线 `refs/heads/main=9065ac1a72b74e2eba6c17d362df243c381b746f`，不以根 checkout 的其他分支冒充 main。
- 冻结设计 v3 SHA256：`22dfed9ad2f9b2e1ed2ac694a359d8f1153ee342ae4d85d35bfef987ad0dee5a`。
- storage 中间提交 `ba696ded` 已获未实施者 `sol61_branch_task_inventory` 独立批准；完整 D1 接线仍在预审，尚未完成最终冻结。
- D1 仅恢复 player 原始 Submitted、typed Voluntary 与真实 Quote、失败零泄漏。D2/D2b 的成交与默认 retail、D3 的 NPC 真实归因/trace、D4 的竞价/Drain、D5 的原 27 integration 整体验收都未完成，不单独合 main。

## 第一轮真实构建

- target 通过 `cp -a --reflink=always` 从既有 feature target 严格 COW 复制，源 target 只读，不允许 full copy 回退或 hardlink。代表性 rlib inode 已核不相同。
- `cargo check -p engine --lib --tests --features simulation-diagnostics -j 8` 在外部 300000ms supervisor 下失败；两项新测试 closure 推断错误已改为显式 `u64/u32`。另一项是 main 原有 `plan_owner_tests.rs:614` 漏 `#[cfg(feature)] gross`：按同 fixture 真实 950 分×100 股 checked `mul_shares` 补齐，保全部旧断言，不把这个编译错误记为 D1 TDD 红。
- 第一轮 4 binary 预构建成功：lib-test、`causal_diagnostics`、`runtime_diagnostics_d1`、`runtime_diagnostics_contract`，`-j8`、独立外部 300000ms，Cargo 报告 2m00s。四个 artifact 都 `fresh=false`；`build-1-source-before.json` 前后 SHA 一致。
- 本轮资源 sampler 启动时 build 已结束，因此没有有效的运行中 CPU/线程样本；实际 `-j8` 命令与 source/artifact 身份有记录，不宣称本轮已证明实际满核。
- 输出均在仓库根 `.tmp/runtime-diagnostics-d1/`，未修改其他冻结 target 或 main。

## 第一轮实际测试矩阵

每条命令受 `run-with-deadline.mjs 10000` 监督，普通 case 全部低于 10 秒；独立 case 并发 8。以下对应 build-1 源码，之后新增两个失败/消费边界测试尚未重新编译，不冒称通过。

| 验证 | 结果 | 证据 |
|---|---|---|
| 原两拍强断言 | 通过：100/100/0、Voluntary、8 market minutes、480 civil seconds | `original-two-step-1.log` |
| 原 restore 观测中断 | 通过，保持 RestoredObservation | `original-restore-1.log` |
| 新公开 D1 原qty/OrderId/Quote/跨 session checkpoints | 3/3 通过 | `public-d1-1.log` |
| 新 session 失败/Seal/unknown 5 项、world raw/Abort 2 项及过滤命中的既有 3 项 | 10/10 通过，2.89s | `unit-d1-1.log` |
| 同固定资源/事件/恢复合同（feature on）及 helper 负控 | 4/4 通过 | `feature-contract-1.log` |
| 精确全簿 Quote 短负载：单股 8 档、8 申报、16 次 Quote | 通过，3.422989ms；明确 O(本股挂单数) | `public-d1-1.log` |
| 原 causal binary 全 15 case | 9 过、6 红，无超时 | `causal-matrix-1.json`、逐 case 日志 |
| 原另两个 integration binary 共 12 case | 尚未用本次源码构建/复跑 | 不沿用旧二进制冒新证 |
| feature off absence + 相同固定操作合同 | 待实际 default 构建与执行 | 未计通过 |

原 causal 六项仍红：`large_real_fill_stream_does_not_reject_recovered_position_cost`、`npc_execution_fixture_has_real_bilateral_fills`、`npc_execution_reconciles_every_share_and_preserves_provenance`、`real_fill_stream_rejects_duplicate_fill_and_wrong_execution_price` 现在明确在 prepare 处拒绝 `NonPlayerUnresolved`，按本中间设计回滚；`fixture::trace_queries_are_read_only_and_keep_per_npc_history_bounded` 仍缺真实 trace；`closing_auction_remainder_has_explicit_day_end_not_voluntary_cancel` 仍缺 DayEnd 终止。原始 27/16 红证不删除、不弱化。

## 必要性与剩余门禁

- source 在真实 external/plan 构造点区分，不能从 Accepted.remaining 或最终 PlanBook 猜；非玩家成功 CancelAll 也保留 unresolved raw，未实施 reviewer 已确认属于冻结设计的必要收口。
- sideband 经 DeskReport→TickEvidence→TickRecord move，driver 在克隆普通审计记录前取走；最外全部成功后才发布 collector，失败不 append。
- Quote 先在委托/撤单修改前验深度及新申报增量上界；随后撮合/删除只减量，避免新增可失败 hook 在业务修改后制造未回滚状态。
- 首轮通过后补充 frame 入口 unknown 不调用 callback/丢弃 driver、成功消费后 report_log 无重复副本两测试，及 owner 局部坐标校验；等待下一实际编译。
- D0b 必须先独立核定非空 Decision/Filled maps、128 trace 环的共享/增量恢复，之后才可实施 D2/D3，不能用 D1 空 map 表现代替性能证明。
