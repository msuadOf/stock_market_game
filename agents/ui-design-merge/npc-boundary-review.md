# UI 分支合并的 NPC 日界独立复核

## 范围与依据

非作者 reviewer 核对 `7d9e8a4` 与 `6c42c69` 两个 parent 的 NPC 准备、消费、隔离提交与恢复差异，以及合并工作树的实际调用链。已读 `AGENTS.md`、`docs/principles.md`、`docs/open-questions.md`、ADR-0025 和两个分支原有日终复核记录。这里只审查内部生命周期，不把 UI 分支优先解释为获准覆盖现行交易制度。

## 语义与必要性

合并保留 feature 的 `GameSession::new` 不提前生成批次、最后市场 tick 不准备下一日请求、日界 `None` 合法恢复与 `plan_tick` 内惰性准备。日内仍由前一提交版本生成请求、下一市场 tick 受理；日界首 tick 使用已经提交的自然日经营与披露版本，并在隔离 `TickShadow` 内、`ExpiryShadow` 之前准备。准备及后续失败丢弃整个 shadow，不能消耗权威 attention、RNG、策略状态或入队 receipt。

同时保留 main 的 `stream_npc_decisions`、实际完成顺序和 `ReceiptBearingIntent` 的账户／股票入队序号。feature parent 的按 AccountId 规范收集属于旧分叉，不应覆盖当前实际先到先受理契约；来源身份不授予交易优先级。当前工作树仍使用 main 的 streaming 和 per-account projection，不增加第二套生产 NPC 计算路径。

这条链不改变 A 股 T+1、委托当日有效、撮合价格优先／时间优先、费用或公开披露时点。依据为 ADR-0025 的完整日级档与既有交易日失效边界，不新增或冒称核验新的交易所条款。公共恢复仍拒绝日内活动委托及非空待处理意图；低层恢复仅在 `day_tick == 0` 接受缺失批次，日内 `None` 与错误 observation tick 继续显式报错。保留个人经历、跨日计划及已发生 receipt，没有先生成后清空、回拨随机流或兼容迁移。

## 发现与修复复核

- 原 main 的两日 archive 测试仍 `unwrap` 日终 `Some(empty)`，与选定的 `None` 日界生产契约冲突。owner 已改为严格 `None` 与正确市场 tick，保留原来的真实 NPC 受理、公共恢复完整深等、后续 observation、attention／RNG 与 receipt cursor 不变量；新增最后 tick 入队 cursor 不变断言，未把真实成交 receipt 错当入队 receipt。
- 旧 helper 的 unconsumed guard 测试依赖 new 自动准备批次；合并后 new 为 `None`，宽泛 `is_err` 会让非日界错误冒充目标 guard。owner 已先调用真实准备函数，再断言精确的 unconsumed 错误与完整状态不变，未削弱断言。

已亲读上述修复后的源码，两项静态 finding 关闭。`queue_empty_npc_at_day_end` 已无生产 caller，保留为既有纯 helper 与其 guards，不作为新日终生产行为的证据。

## 短测门禁

Reviewer 已亲读当前合并版 `base-host-build2.stderr` 的成功编译结尾、真实 artifact 清单 `base-binaries.json`、`core-short/results.log` 及全部 11 份单 case 原始日志。Engine binary 为 `engine-894d4dae67cabce7`，Desktop binary 为 `stock_market_game_lib-069ec2baafa6ce96`；每份日志均为 `running 1 test`、目标 full test name 的 `ok` 与 `1 passed; 0 failed`，没有把 `--list`、过滤后零 case 或编译通过当作测试通过。

Engine 8 项覆盖 main 的真实两日 archive、feature 的休市／交易日 continuation、真实 due 日界准备及失败回滚、日内缺批拒绝与旧 helper 三项 guards；Desktop 3 项覆盖权威 CivilDate baseline／restore、共享 ingress 无需 actor command polling，以及恢复切换 generation 并关闭旧 ingress。作者记录整批外部 10000ms、child 9000ms、6 个进程并行、每进程 Rayon 4，实际 1.56 秒；单 case 日志最大 1.36 秒。初次 shell quoting 失败留在 `invalid-runner-results.log`，没有有效业务 case，不登记为业务红灯或通过证据。

当前 diff 继续保留 main 的实际完成 channel／receipt 顺序实现。Reviewer 另亲读 root 运行的 `npc-completion-channel-short.log`：`session::pipeline::npc_decisions_tests::` 实际 20/20 通过、0.20 秒，外部 10 秒、test threads 6／Rayon 4。其中包含 fast account 先完成及失败后 drain、玩家请求插入 fast／slow NPC 完成之间、单 Rayon worker 无死锁和双 worker 多会话准备，并保留本人风险、T+1、真实 arrival 与个人 analysis 边界。此为代表性并发短测，不等于穷尽调度轨迹。单账户 continuation 的完整状态相等不外推为多账户自由并发的未来字节确定性；三账户测试只要求已发生事实与合法 cursor，不人为保证每次观察必然下单。

**结论：本次 NPC 日界合并范围 PASS。** 两项有效 finding 已修复，当前源码与上述代表性短测足以关闭本范围的语义、必要性和边界门禁。Reviewer 仅核对真实日志，未另启 Cargo、复杂回归或网络验证。此结论不代表整个 UI 合并、恢复先前未提交功能或全部 checklist 已完成；后续 stash 恢复改变相关源码时须按实际 diff 再次复核。

当前 `cd595607` 后恢复的 WIP 已作增量静态核对：`new`、`plan_tick`、最后 tick 的 deferred `None` 三段未被覆盖，两日 archive 的严格 `None`／入队 cursor／attention／RNG 测试保持。新增个人 Fill 交割记录、月报 scheduled 公布与真实 Trade 分钟记录均位于隔离 candidate、最终提交之前；分钟标记使用 authority 的实际成交时段而不是 rollover 后的新日时钟。自然日日结完成更正、经营／封账／披露与历史归档后才捕获公共候选；日内 active history 被公共恢复拒绝，不靠清空档字段掩盖。

Protocol 的新增 publication transaction 把共享 ingress 自然日发布及 disclosure observers 延迟到成功外层提交；失败和 Drop 恢复 checkpoint，旧 publication／scope 不允许回滚已发布日期。`publish_calendar` 在全部校验及 checked epoch 计算成功后才修改共享状态，没有失败后半发布。新增历史查询转发不会另行准备 NPC 或从持久档恢复活市场。main streaming 保留；`npc_tick_preparation` 新增的等待／记录 gate 只属于 `verification-harness`，非 verification 生产不增加 gate。

这份增量核对未发现 deferred 日界被反覆盖或新增交易语义漂移，但不代替各功能完整复核。此前 11＋20 项绿色属于恢复 WIP 之前的 base binary，不能证明这些新增字段、交割、披露、历史与 publication transaction 已在当前 WIP 通过。当前增量运行门禁等待统一 fresh 构建及短测；不将原 base PASS 扩大为当前 WIP 全部完成。
