# Rust server 本批独立复核

复核日期：2026-10-06。基线 HEAD：`865eaa23fce6cf21853a6c6449055018f743b6c7`。复核者未参与本批实现，仅读取指定未提交源码 diff、两个新增完整源码文件及必要的调用上下文；不审查已提交历史，不实施源码修改。本报告是本轮结论，不包含后续修复复核。

## 发现

### P3：新增 serde 对照只比较了两个失败结果

位置：`packages/engine/src/session/pipeline/ledger_paged_map.rs:307`。

`ordering_and_serde_match_btreemap_for_account_and_key_boundaries` 把完整 struct `EnvelopeKey` 用作 JSON map key；`serde_json::to_vec(&rows)` 与 `serde_json::to_vec(&expected)` 都返回 `key must be a string`。将两侧错误转成字符串再比较，只证明这类 JSON 错误保持一致，不能证明成功 Serializer 收到的 map entry 内容和顺序一致。

这仅是测试证据缺口，没有发现 `Serialize` 实现的功能错误。`iter()` 与 `BTreeMap::iter()` 的全量严格比较已经覆盖实际 canonical 顺序；实现本身逐项调用 `serialize_entry`，也没有观察到内容遗漏。建议显式保留 JSON 非字符串 key 的失败契约，同时用支持结构化 map key 的测试 Serializer 比较成功序列化条目，避免把相同错误作为成功序列化证据。无需修改 `EnvelopeKey`、生产 wire 契约或新增核心依赖。

未发现 P0、P1 或 P2 生产正确性问题。上述 P3 应补强后由另一名未实施修复的 reviewer 复核，本报告不替代该复核。

## 固定清单结论

1. **ledger 分页：通过源码检查，serde 测试证据有上述缺口。** `EnvelopeKey::Ord` 的首字段是 `account`，group/page 按其高位单调分区，页内仍按完整 `(account, stock, order, side)` 排序；没有将分页布局作为交易优先级。正向迭代及拥有型迭代与 `BTreeMap` 完整比较，hash projection 仍读取同一 canonical iter。固定位置使相同内容有唯一页布局，`PartialEq/Eq` 保持值语义。`SharedRows`、group、page 三层 `Arc::make_mut` 隔离候选修改；删除清理空页/空 group，`retain` 在删除每行时扣减 len，不依赖缩小后的 len 定位旧行，末尾多行删除不会触发原先的定位 panic。
2. **稀疏 rebase：通过。** 修改前调用完整证据检查；缓存仍覆盖四个 map allocation identity，任何通过 `SharedRows` 的可变访问都会使缓存 identity 失效。已有行只重置 `origin/basis/spent/released`，不修改累计 `audit` 或全局 `next_receipt_index`；终结行的 audit/conservation 按完整 key 删除，存活的非空 conservation 清零，局部 receipt 身份清除。`basis == live` 结合非负守恒已蕴含 spent/released 为零，因此未变化页 fastpath 不漏 reset。公开 rebase 先构造 candidate，失败不替换原状态；private rebase 的失败由 CommitTick owner 丢弃完整 candidate。未把 private 原地接口误报为自身原子接口。
3. **NPC 消费与受理：通过。** 外部 host 用 `in_place_scope` 在 caller 消费，worker 执行 NPC 任务；native Rayon worker 调用保留独立 consumer，并让 `rayon::scope` 的等待具备调度机会，避免 consumer 占满 pool 后阻塞待处理 NPC。单 worker 和 WASM worker 保持已有 inline 分支。完成即接收、Player 在快慢 NPC 之间插入、consumer 失败后 drain、稳定错误选择逻辑均未改动；对应测试通过。有限测试不证明所有可能嵌套执行绝对无 deadlock。
4. **intraday 与回滚：通过。** `Arc<TickFrame>` 只改变 ProtocolState 的内部存储。追加到共享未满 chunk 时仅复制 frame Arc，之前的 events/facts/payload 不被修改；回滚还原 checkpoint 持有的旧 tail。CivilUpdate 构建时显式恢复 `Vec<TickFrame>`，`step_frame` 的返回类型和 TickFrame wire 字段未变；Arc serde 透明地序列化内部 frame。新增 33 帧未满尾 chunk 的地址、rollback、retry 检查与已有跨日回滚测试通过。
5. **玩家与完整 snapshot：通过。** 高频路径在计算资源预留前按 owner 过滤，仅处理玩家订单，并直接查找玩家账户；完整 snapshot 仍处理所有 NPC 和玩家。借用订单遍历与旧到达顺序遍历虽然枚举顺序不同，但此处只累计非负现金/股份，没有将遍历顺序用于撮合。原 `filled_value` 与 `live_cash_reservation` 调用保持不变，部分成交费用增量、T+1 锁定字段和卖单预留股数不变；玩家与完整投影一致性、部分成交及溢出检查通过。
6. **server 探针：通过，证据范围明确。** 新增 example 通过生产 `SessionManager::default_base()` 新建 SessionActor，用 `+Infinity` 选择 Fastest，并使用真实 broadcast。逐 frame 检查 tick 从 1 连续递增；达到请求 tick 数后仍完整计算最后一个 batch，输出实际 committed 数量，未隐藏 batch overshoot。默认 `BASE_TICK_MS = 1000`，故 `committed / wall_s` 对应宿主 1 秒一 tick 基准的倍率；不应外推为自定义局的真实 A 股日历速度。计时在 running 命令前开始，初始化另计，包含休市自然日推进和广播消费，排除后续 metrics 查询、停机以及 HTTP/WS 序列化和前端。配置 worker 数不等于有效 CPU 核利用率。
7. **fixture 必要性：通过。** finalizer fixture 只固定项目模拟日历中的交易起始日；acceptance fixture 只恢复其现金断言所依赖的 10000000 分（10 万元）开局。两个 integration fixture 分别补齐已使用的 `WithinKindDistribution` import 与 `PersonalHistoryReadLedger` 变量。原断言均保留，没有弱化交易、费用、T+1 或回滚要求。主 agent 关于四个扩大回归失败也存在于独立 HEAD archive 的结论属于其提供的证据，本 reviewer 没有重新编译该 archive。

## 三项项目门禁回答

**大 A 语义及依据：** 未发现本批改变价格时间优先、费用累计、资金/股份守恒、T+1、申报单位或交易阶段。判断依据为实际 diff、既有 `docs/trading-rules.md`、ADR-0017、ADR-0018 已获批准的局部冲突条款以及 ADR-0032；这些文件明确保留来源不决定优先级、提交失败丢弃 candidate 等约束。交易规则基线记录的官方核对日期为 2026-09-22/25，2026 版规则适用日期为 2026-07-06，费用表访问失败已有明示。本批没有修改交易制度，因此没有进行新一轮官方规则获取，不把此次工程复核表述为全部现行交易制度的再次官方验收。2030 年 fixture 日期属于项目模拟日历，不是未来交易所休市安排的官方证明。

**需求必要性与最小范围：** 本批集中于生产 Rust server 热路径中 ledger 复制/构建、NPC consumer 线程、checkpoint frame 深复制和玩家资源投影，新增 probe 用于验证生产 Fastest。没有改资金配置的生产默认、降低 NPC 数、跳 tick、删除守恒检查、引入额外依赖或扩大到 frontend。fixture 修复是恢复相关回归检查所必需。两级分页的复杂度与 COW 复制范围对应，未见额外业务政策；页按账户范围组织，并不保证每页订单数量固定有界，不应把该实现宣传为解决任意长跑规模的全部成本。

**边界测试、跨层语义漂移与复杂度：** 仅发现上述 P3 serde 测试证据缺口。完整 key 顺序、末尾 retain 删除、COW 隔离、累计费用与 cursor、完整证据拒绝、失败原子性、worker 并发 session、Player 插入、drain、共享尾 chunk rollback、部分成交预留与溢出均已有有效覆盖。本轮未复核 WASM runtime、HTTP/WS 与前端整体运行，不据此宣称三宿主全量验收完成。性能报告约 100.81x 且 1000x 尚未达标，与源码探针范围一致；本 reviewer 未重跑性能测试，也没有认证该目标完成。

## 本轮实际验证

使用主 agent 指定的现有 `.tmp/perf-tests/debug/deps/engine-3e4dbe3a7e208ca7`，四个独立 filter 并行执行，每条命令均使用 `timeout --kill-after=1s 9s` 和 `--test-threads=4`：

| filter | 结果 | 测试进程报告耗时 |
| --- | --- | --- |
| `ledger_` | 72 passed，0 failed | 0.61s |
| `snapshot` | 42 passed，0 failed | 1.72s |
| `npc_decisions` | 21 passed，0 failed | 0.22s |
| `rollback` | 29 passed，0 failed | 3.51s |

filter 存在重叠，不能把表中计数当作唯一 case 总数。本 reviewer 未重新构建 binary；其源码绑定由主 agent 的构建记录负责。未执行性能基准、integration 重编译或全量验收。除本报告外未主动写入任何工作文件或源码。
