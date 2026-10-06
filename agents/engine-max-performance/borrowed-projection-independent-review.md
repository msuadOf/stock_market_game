# Borrowed Envelope 投影独立复核（2026-10-06）

结论：**通过本批静态独立复核，无阻塞发现。** 审查者未参与实现，未启动构建、测试或基准，未创建其他 agent；只写入本报告。

审查范围为 HEAD `8d5ed41b39f73bd23bd3e07c029e2e545ec59d43` 上尚未提交的全部 9 份生产/测试文件：`envelope_projection.rs`、`envelope_projection_hydration_tests.rs`、`pipeline/continuous_matching.rs`、`continuous_matching_adapter.rs`、`continuous_matching_adapter_tests.rs`、`envelope.rs`、`ledger.rs`、`ledger_state_tests.rs`、`mod.rs`。完整 diff 共增加 447 行、删除 59 行，SHA-256 为 `9ab2fd934f39fd784efa9978df7c49f9ba7f94899f4bcdc606ad14e3af020af7`。此前已提交的性能改动和撤回实验不属于本次结论。

已阅读 `AGENTS.md`、`docs/principles.md`、`docs/trading-rules.md`、`docs/open-questions.md`、`docs/architecture.md`、ADR-0017/0018 中资源、费用、失败原子性与局部受理相关条款，以及 `borrowed-envelope-findings.md`。

## 1. A 股语义与依据

本批符合既有 A 股基线及已登记游戏简化。`envelope_projection.rs:194` 的借用计算仍使用原 `buy_order_reservation`、`cumulative_nominal_fees` 和相同参数/错误转换，金额仍为分、股份仍为股；卖单 cash 仍为零。`envelope_projection.rs:103` 先核对 nominal，再运行原 charged 门禁，保留权威 ledger 的卖方逐腿封顶与后腿追收历史，没有重新计算或丢失费用债。

`envelope.rs:47` 的身份元组与 `receipt_key.rs:98` 的 `EnvelopeKey::Ord` 字段顺序及 Buy/Sell rank 一致。`envelope_projection.rs:61` 先收集与旧 `resting_orders_matching` 相同的 bids/asks 引用，再按相同 `seq` 稳定排序；随后才按完整身份进行审计，因此没有把比较顺序变成交易受理优先级。连续簿、竞价队列与跨股遍历的费用计算首错顺序保持一致。

`envelope.rs:75` 显式解构所有 Envelope 字段，比较结果等价于 `tick_start_existing` 生成值的完整 Eq；`ledger.rs:497` 先检查 terminal 冲突，随后检查长度、map key 和 Envelope 自身 key。已有 ledger 的借用门禁只读；`envelope_projection.rs:42` 中空 live ledger 仍进入原 owned hydration 与缺失卖方历史拒绝路径。未改变 serde、hash、receipt cursor、校验门禁、SealAllocationSnapshot、释放可见性或 CommitTick 原子边界。

依据是已接受 ADR-0017 的资源/费用/失败边界、ADR-0018 的局部冲突约定及仓库已登记的官方规则记录，足以支持此次等价优化。**本次没有重新访问交易所或中国结算来源，不声称重新核验官方费用**；`trading-rules.md` 已记录的中国结算费用表访问失败限制仍存在。

## 2. 必要性与最小范围

改动直接消除已有 ledger hydration 和 Continuous adapter 的重复 owned Envelope/StockCode 构建，没有增加依赖、持久缓存、共享权威 payload 或第二套费用算法。`continuous_matching_adapter.rs:76` 在单股范围复用 lookup key，并以 account/order/side 去重，stock 恒定使身份范围等价；`:107` 仍计算完整费用 facts，`:124` 仍克隆权威 Envelope 形成独立 stock snapshot。`continuous_matching.rs:880` 只复用单股 lookup key，保留原校验及其顺序。新增计数器仅存在于 `cfg(test)`。范围与所述瓶颈相符，未发现无关交易修改。

## 3. 边界、漂移与复杂度

新增逐字段测试将所有 key、资源、origin、费用 audit 和 pending_price 变体与 owned Eq 对照；`ledger_state_tests.rs:871` 的 21 组精确 Result 对照覆盖资源/audit 漂移、缺失/额外/重复/空投影、terminal 冲突和 map key/Envelope key 不一致。原 hydration 测试继续覆盖卖方历史欠费、路径相关 rounding、nominal 漂移、缺失 charged 历史和失败时 ledger 不变。未发现跨层契约漂移或阻塞性的复杂度问题。

唯一非阻塞建议：`envelope_projection_hydration_tests.rs:312` 可另用一个短 fixture 固定完整 hydration 的混合错误首错，例如簿内 `seq` 与身份顺序不同、一个身份缺失而另一个 nominal 异常，同时存在 terminal 重叠。当前 21 组对照只直接测试 ledger 门禁，不覆盖前置费用计算与 charged 审计的组合顺序；本次已通过逐分支静态对照确认实现等价，此建议不阻止通过。

## 验证限制

TDD 红绿、已有短测和性能数据来自实施记录；root 另告知最终 source binding 与 hydration/borrowed/adapter/ledger 短测通过，本审查未独立执行或核验其运行日志。本结论只确认上述完整 diff 的静态语义与范围，不代替 root 的最终绑定验收，不证明长期稳态、WASM/Web/HTTP/WS、完整 workspace 或全交易日验收通过，也不宣称达到 1000x。
