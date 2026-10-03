# pipeline state caller 迁移第一组

## 范围与约束

仅处理 `/tmp/pipeline_remaining_1.txt` 分配的 36 个文件。已读取 `AGENTS.md`、`docs/principles.md`、`docs/open-questions.md`、`docs/architecture.md`，核对相关 ADR 路径；分配源码已阅读全文，对工具输出截断部分进行了补读。未新增依赖、未引入交易规则变化。

## 迁移内容

- GameSession 原私有状态调用转为 `state` 成员，保留 facade 方法、错误注入字段和 DTO 成员。
- Account 与 Position 只读调用改 getter；测试账务/类型/策略写入使用 `fixture_set_*`，现有值、断言和 T+1 fixture 边界保持。
- NPC attention fixture 改用唯一 `attention_scheduler.enqueue/clear`。
- ParentOrderPlan 测试构造改 `from_facts`，保持原事实。
- SessionExecutionTransaction 传入唯一 `belief_participants`，对既有 validated participant 写回 `belief_mut`，保留 information/watchlist/price_memory。
- TickShadow 中跨 `Option<GameSession>` 的链式 state 与 strategy getter 一并迁移。

## 核验边界

对 17 个实际迁移文件执行定点 `rustfmt --edition 2021 --config skip_children=true`，返回 0。静态扫描未发现旧 GameSession 直接私有字段访问、旧 attention_queue/belief_books、ParentOrderPlan struct literal、重复 state 或重复 getter 括号。剩余 `cash/kind` 等字段已逐项确认为 ResVec/SelfView/EnvelopeReceipt 等 DTO。

依父协调者约束，未执行 cargo、测试、Git 或全仓格式化。编译与回归尚待统一验证；独立复核由父协调者统一安排。本记录不宣称整批实施完成或测试通过。

## 最终文件版本

| 文件 | 行数 | SHA-256 |
|---|---:|---|
| `packages/engine/src/session/pipeline/account_validation_context_tests.rs` | 47 | `cf9264178f8d7382aa21ed2fec36acf06bfaa4885026854d12c166d2dbf56705` |
| `packages/engine/src/session/pipeline/account_validation_driver_tests.rs` | 686 | `6e636213f2f16166075142e814ceea7d1790279810595bd4246cb511dbbb08ff` |
| `packages/engine/src/session/pipeline/authoritative_tick_tests.rs` | 521 | `a7db2ccc504c7e71d613dd64c8c4162be3609e312d8ba5af1ad3373104cd6ab0` |
| `packages/engine/src/session/pipeline/candidate_commit_tests.rs` | 135 | `0944b40dee9149034848d19b7a2d6afdd3fa097533095084159631c4c73a8cdd` |
| `packages/engine/src/session/pipeline/candidate_composition_tests.rs` | 77 | `e87ac2d0a971143957bd66a5a17c4c8fc85030d4f7727baacd6ba253c34f23ea` |
| `packages/engine/src/session/pipeline/commit_evidence_tests.rs` | 421 | `f8e5bfe8a4f3373a796fd195aeda47eb2215db982053a13abc6f3dbc9e2c8108` |
| `packages/engine/src/session/pipeline/continuous_matching_adapter.rs` | 142 | `eaeb7f8c08aa6cda8dbbdd5edd846395a5b4b716fc1440dbfb018fa7c712b425` |
| `packages/engine/src/session/pipeline/continuous_projection_transaction.rs` | 29 | `2f2769ebaff76e9c39b4393e4f50ba942cfacc81bcc239e994d9049c4822b3fe` |
| `packages/engine/src/session/pipeline/continuous_tick_transaction.rs` | 364 | `f8fb6af3d25d8bcd83180c8066bd608b607ca1263b4315c72742bf085e022991` |
| `packages/engine/src/session/pipeline/continuous_trade_acceptance_tests.rs` | 555 | `7f81f8db08517bd2e77797ae2a2730186a7917330460221ab2759dc483b76839` |
| `packages/engine/src/session/pipeline/envelope_tests.rs` | 145 | `242dbf88441dea8e53d578ef4ef42f5a2ac0a1be2bcb0bb00d4615997976c449` |
| `packages/engine/src/session/pipeline/event_collection_tests.rs` | 119 | `990ae2a6be183d746df41e4bcf3f137b99cdd807efd4fa67ba9842c601b94f28` |
| `packages/engine/src/session/pipeline/execution_fact_producers.rs` | 176 | `f0994b24451ff756b2a5fdfdea06a60f0b0257ff36f9017a0e62e23553824630` |
| `packages/engine/src/session/pipeline/executor_perturbation.rs` | 148 | `509f8858c4394212e1639e8e432b4586edd461a5d1f02ffe2da55335fdbf8cfb` |
| `packages/engine/src/session/pipeline/incremental_auction_round_tests.rs` | 320 | `03ebf581447434c40e5c50bb9c2e1aa9730d28f46b704ecef2b53f87f85c21f8` |
| `packages/engine/src/session/pipeline/intent_candidates.rs` | 300 | `c1c85dfae1112d9a8efa26cd4d596ab7f0e5a28fa148be135b1c4561c8d9a539` |
| `packages/engine/src/session/pipeline/ledger.rs` | 469 | `cc17be56c2b7247f187ff83ee835047ff32dee28bf7358b5d65e73af8f3ba233` |
| `packages/engine/src/session/pipeline/ledger_conservation.rs` | 81 | `d600e5d8185f86189cb7731112dd29f9800fa79f233e809b66f16c4b168d536c` |
| `packages/engine/src/session/pipeline/ledger_receipt_tests.rs` | 634 | `49dfb2c753c692a7deaba7f9fdf693ca5c8318c22a1cc41d5e97c8689c9b75bd` |
| `packages/engine/src/session/pipeline/ledger_tests.rs` | 176 | `938ccadd29d3716d51bb5cadc6bbceacdf90f8054213733834376960425d0227` |
| `packages/engine/src/session/pipeline/npc_decisions_tests.rs` | 592 | `5249260e3584fa6d346e6b919f00ae3169b6c9dd803128fa17cacdf5b94d2ac4` |
| `packages/engine/src/session/pipeline/npc_state_projection_tests.rs` | 502 | `ec1ed87ef9ec654ccbf71a8def86f7e141e8ad0ef0399b88d411f35f785c6182` |
| `packages/engine/src/session/pipeline/npc_tick_preparation_tests.rs` | 586 | `cdd3899acfa52494e947fb731da956db604715b1e86529e1ff350b1972f78885` |
| `packages/engine/src/session/pipeline/pre_open_transaction.rs` | 368 | `c6954295b08ab4bef303854b3a123a0b012f24bb94c6cf2eedb1a96ce90296b6` |
| `packages/engine/src/session/pipeline/price_resolution.rs` | 90 | `4829669367fcbf0058c3564e4096e3eb77df605d682a5dac44076c0f8b9e9c73` |
| `packages/engine/src/session/pipeline/ready_ingress.rs` | 127 | `0e16bf32a1038d2e5d590c49ff8bee8b25d6b22d9fbfefdbea0e9ae76efdaeff` |
| `packages/engine/src/session/pipeline/ready_stock_stream.rs` | 234 | `6c88a0f4e25988c44c797047685f426d210456bbb180657ce9f38ec8640535b0` |
| `packages/engine/src/session/pipeline/receipt_aggregation_tests.rs` | 490 | `823e28c38ae503af92b69d598148d0cfb14ee1232d61fa317a5660e59f0b55b5` |
| `packages/engine/src/session/pipeline/receipt_key_tests.rs` | 187 | `1e7cf98908250d44ce63f5dc5b0027777943cef0cab2f302218f4997f2c2a9aa` |
| `packages/engine/src/session/pipeline/session_execution_transaction.rs` | 161 | `12c6e22c81076c96db5b7155ed10a99f48d2b75a372dc3f86f8af20c1ddd51f9` |
| `packages/engine/src/session/pipeline/session_fact_producers.rs` | 130 | `fd06d3605f2afbfa3c35cfe535e2412b29730942275465202e4d5f5834ae6ec4` |
| `packages/engine/src/session/pipeline/shadow.rs` | 92 | `bb106c7f87b421b131b622dcbecb507b5c828b050ef1b61b598fcdb0e6bfb714` |
| `packages/engine/src/session/pipeline/stock_auction_adapter.rs` | 360 | `9c7c2594a35c105bb8f8cb78bbaf75d6e3e6291a8ed95630d0a04ad962de31bc` |
| `packages/engine/src/session/pipeline/stock_auction_tests.rs` | 788 | `4696e7c5d6dfabe3bfabacdaba2d46881cebfc3e182232e40c245ffe931cc227` |
| `packages/engine/src/session/pipeline/stock_execution_transaction_tests.rs` | 491 | `ef8baa63c35823b4384a45d63b92c0e1e21e97a15fd9491fc6af8f1bc0edd17c` |
| `packages/engine/src/session/pipeline/transaction_error.rs` | 63 | `f9988ce72ae2c19ff4b93136f882cb1b92ccabcea5f7e5902daa70a8aa45f722` |

## 独立复核边界补充

根据独立 reviewer 指出缺少 nonempty belief_patch 路径，追加一个真实机构买入测试。复用本文件的 P3/P4 worker fixture，扩展 buyer 参数并由现有 stock adapter 承接存量 maker envelope；玩家卖出 100 股，机构买入 100 股后经 `apply_session_execution_transaction` 安装 Settlement/Projection。断言真实双边 Fill、机构买入 order/price/minute 与 T+1 锁定，并逐项验证具有显式内容的 information/watchlist/price_memory 和另一机构完整 participant、账户 kind/cash/positions/strategy 不变。未改生产路径。

定点 rustfmt 返回 0；按协调者指示未运行 cargo/测试，尚待独立新增 diff 复核和统一执行。精确 test filter：

`session::pipeline::session_execution_transaction_tests::institutional_fill_updates_belief_and_preserves_participant_members_and_other_account`

新增覆盖文件：`packages/engine/src/session/pipeline/session_execution_transaction_tests.rs`，249 行，SHA-256 `605489a846a5085640299129102364a563d9f870059024b171b79e8135d433c8`。
