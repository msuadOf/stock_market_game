# Arc BookState、strategy presence、ledger 并行与 hydration 独立复核

复核日期：2026-10-06。复核者为未参与实施的独立 subagent；未修改实现与测试，未委派其他 agent。

## 范围与依据

基线为 `3b52f1c2`，审查该基线之后完整 tracked diff：9 个文件、420 行新增、61 行删除。已读 `AGENTS.md`、`docs/principles.md`、ADR-0017、`docs/architecture.md`、`docs/open-questions.md` 与 `docs/trading-rules.md`，并检查受影响函数、Account mutation 入口、OrderBook hash projection 和现有 hydration / evidence / failure 测试。

本批未增加或修改交易制度，复核沿用仓库已登记的现行沪深 A 股规则与游戏简化边界；未重新访问交易所或中国结算材料，不能将本复核称为完整法源重验。

## 结论

未发现价格时间优先、资源方程、失败写入顺序、存档或 hash 的实现回归。首轮发现两项 P2 边界测试证据缺口；第二轮已审查补测并独立运行通过，两项均关闭。本批独立复核门禁通过，不代表性能目标或全仓库验收已经完成。下文保留首轮发现作为历史证据。

### P2：并行 helper 错误序测试不足以证明真实 ledger 的首错契约

位置：`packages/engine/src/session/pipeline/ledger_conservation_tests.rs:127`。

新增测试对 `map_rows` 输入简单整数并返回两种错误，只证明 indexed chunk 输出顺序；没有触发真实 `validate_conservation` 在前行 `accumulate_totals` 的现金或股份 checked overflow 与后行 conservation 错误同时存在的边界。完整 `validate_complete_evidence` 的新增并行行路径也缺少大于或等于 512 行、live 与 terminal 同时存在且存在多种 audit / overlap 错误的直接回归用例。

源码目前按原序消费每行 `Result`，随后立即执行原顺序的 checked 全局累加，因此早期累加溢出仍会优先于后行错误；但将来把 `Result` 提前 collect 成一个总 `Result` 就会破坏该契约，而当前测试仍会绿灯。建议用 512 行代表性 fixture 覆盖 cash / shares 全局溢出优先于后行错误、前行错误优先于后行溢出、live 后接 terminal 的首错顺序，并通过完整 evidence 入口验证大 ledger 的 audit 首错。无需分钟级 fixture。

### P2：hydration 关键拒绝路径缺少直接、等长的边界测试

位置：`packages/engine/src/session/pipeline/ledger_state_tests.rs:121` 和本批新增测试。

原 `hydration_rejects_every_mismatch_without_mutation` 用两行 authority 对一行 resource / audit / origin mismatch；这些输入先被长度判断拒绝，不能独立证明新 `zip` 分支逐项比较所有 Envelope 字段。session 层已有同长度 quantity 与 fee history 漂移测试，可提供部分补充证据，但尚无直接 terminal overlap 的 empty / nonempty ledger hydration 回归用例。

建议增加同数量、同键的 resource / audit / origin mismatch、同数量替换一个 key、cold empty 输入重复、empty 与 nonempty 路径的 terminal overlap，并检查错误描述与完整 ledger 无 mutation。当前实现保留原来的 duplicate → terminal overlap → equality 首错顺序，冷路径仍调用 `EnvelopeLedger::new`，未发现实现差异。

## 语义与范围核对

- `BookState` 仅将 bids / asks owner 改为 `Arc<BTreeMap>`；价格时间键、遍历顺序、live / filled identity 与 hash projection 未改。`apply_maker_fill` 仍先减 `qty`，再 checked 更新 `filled_qty`，维持 partial maker overflow 的既有局部写入行为。COW 隔离了旧 authority；`clear_resting` 换空 Arc 与原清空结果一致。
- strategy presence 是页上派生 `OnceLock<bool>`。`get_mut`、`values_mut`、`insert` 均先失效缓存；`Extend`、`FromIterator`、T+1 解锁复用这些入口；共享页 Clone 与发生 COW 的 AccountPage Clone 分别保留/重置应有缓存。Account `kind` 和 strategy Option 没有可通过共享引用改变的入口。
- ledger 行验证仅读不可变行，Rayon indexed `par_chunks(64)` 保留原 live BTree 顺序后接 terminal BTree 顺序。全局五项 `ResVec` checked 合并顺序与旧代码一致，未使用重排的 parallel reduction。完整 evidence 的前置 conservation、零资源谓词和行数检查顺序不变。
- hydration 排序 Vec 的 duplicate 与 key/value 比较与旧 BTreeMap 构造后的 equality 等价；未重置 nonempty audits / conservation，不更改 receipt cursor。冷 hydration 仍构造三份正式表。
- 本批没有新增依赖或跨层接口；CPU probe 增加指定 binary 与 SHA-256，browser probe 增加真实 threadCount 参数及记录，属于性能证据工具的必要范围。未在本复核执行 browser 或性能 benchmark，无法验证性能收益，更不能据此宣称达到 1000x。

## 独立验证

使用主 agent 提供的已编译测试 binary `.tmp/perf-tests/debug/deps/engine-6ae5d5d49d140673`，SHA-256 为 `9590b7234240f239b87c034c44d732d8cb7205a1b4d7d6b07d1ba93fb9f2976e`。本复核未重新编译，source 与 binary 的对应依据是主 agent 报告的 `hydration-tests-build.log`，不是独立编译证明。

每条命令使用进程外 `timeout -k 1s 9s`、`RAYON_NUM_THREADS=4` 和 `--test-threads=4`；独立套件并发运行。结果：

| filter | 结果 | binary 报告耗时 |
| --- | --- | --- |
| `ledger_` | 64 passed | 0.55s |
| `orderbook::` | 11 passed | 0.00s |
| `session::account_book::tests` | 5 passed | 0.00s |
| `session::failure_tests` | 6 passed | 0.44s |
| `session::envelope_projection` | 10 passed | 0.56s |
| `session::hash_contract_tests` | 14 passed | 0.95s |

最初 failure filter 误用 `session::failure::tests` 命中 0 tests，随后明确更正并运行真正的 6 个 failure tests。总计 110 个测试执行，其中 1 个 projection 测试与 ledger filter 重叠；没有把 0 tests 当成通过证据。未执行全仓库回归、WASM/browser 重构后验收或 ≥1000x 目标验收。

## 第二轮复核与发现关闭

仍于 2026-10-06，由同一未实施者审查基线 `3b52f1c2` 后最新完整 tracked diff：9 个文件、582 行新增、61 行删除。新增部分仅为三个必要边界测试，生产实现与首轮审查一致，没有扩展交易规则或接口。

第一项 P2 已关闭：`real_parallel_ledger_reports_early_total_overflow_before_later_row_failure` 使用真实 512 行 ledger，在 cash `i64::MAX + 1` 与 shares `u32::MAX + 1` 两种路径上，分别用 1/4 worker 验证早期全局累加溢出优先于第 500 行缺失 conservation，并检查 ledger 不变。`full_evidence_parallel_threshold_and_mixed_terminal_rows_preserve_first_error` 从真实 expiry release 与 remove_terminal 建立有 terminal 行的完整 ledger，原始 511/512/513 行均先成功验证；随后增加较晚 overlap 错误与较早 audit mismatch，1/4 worker 都返回 audit mismatch。注意制造 overlap 后实际行数成为 512/513/514，所以 valid 状态检查覆盖 511 的串行路径，错误竞争检查均覆盖并行路径；未将这些竞争结果错误表述为 511 行串行竞争测试。

第二项 P2 已关闭：新增 `hydration_rejects_same_length_field_key_and_terminal_mismatches_atomically` 覆盖等长 resource、audit mismatch、等长替换 key、cold empty duplicate，以及 empty/nonempty ledger 的 terminal overlap，并核对失败无 mutation。origin 仍由结构完整的 Envelope equality 比较，相关生产代码未改变；没有为了关闭发现无限扩充所有字段组合。

独立重跑使用相同外部 9s TERM + 1s KILL deadline、4 个 Rayon worker 和4个测试线程，两个独立套件并发：`ledger_` 为 67 passed（0.56s），`session::envelope_projection` 为 10 passed（0.54s）。最新 binary SHA-256 为 `4793b8b94b5ed38036503765f83e88ddfad71313d2365d90474cb732b2893f5e`；依然未由复核者重新编译。亦已读取主 agent 提供的三个短测 green log，均确有一个目标测试通过。

跨层再次核对：公共 `hydrate_or_validate_envelope_ledger` 仍独立投影挂单事实并校验 nominal / charged 卖费历史；OrderBook 的内部 Arc 不进入 hash projection 或存档契约，resting orders / restore 仍使用实际 Order 和序号事实；AccountBook 页缓存没有进入序列化或业务 hash。本批没有 SourceGate/source_gate 字面类型或新增 gate，本复核按 source、engine、hash/save/restore 与宿主接口的依赖链理解该核对要求。主 agent 报告最新 WASM 编译和 threading check 已通过，本复核未独立运行二者，不能将其算作复核者独立测试。

最终无未关闭有效发现。此结论仅批准本批语义、必要范围及已覆盖边界，不宣称 ≥1000x 或整项性能工作完成。
