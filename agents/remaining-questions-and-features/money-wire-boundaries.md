# Q01 Money 边界测试与当前 fixture

本批只同步当前测试中的 Money wire representation，不改变 Rust 内部 i64 分账务、A 股交易制度、费用计算、申报股数、seq、公共交易 tick 或 ratio。Money 的权威输入仍由 Rust 严格 decoder 校验；没有 schema 代际、旧数字兼容或迁移路径。

## 单位核对

- `SessionSetup.stocks[].initial_price` 是每股整数分，`stocks[].tick` 是价格最小变动整数分，因此 fixture 分别传 `"1000"`、`"1"`；这不同于 `PublicTradingSeconds.tick` 的时间 tick，后者保持数字。
- `npcs.retail_cash_median`、账户 `cash`、`GameConfig.commission_min` 与 `starting_cash` 均为整数分，fixture 与 JSON 输出断言改为规范十进制字符串。
- `LimitPrice::Fixed` payload 是 Money，`"1000"` 表示每股 10 元；`SpeedRequest::Fixed` payload 表示速度，仍是数字。
- `AuctionTick.indicative_price` 是 Money；同事件 `seq`、`tick`、`matched_volume`、`imbalance` 均保持原来数字语义。
- `SavedLiveEnvelope.charged` 的 commission、stamp_tax、transfer_fee 均为 Money。非法额外派生字段测试先成功解码当前合法 baseline，再注入字段，避免因为错误 Money fixture 意外通过拒绝测试。
- AccountingAmount 的公司账务元编码、股数与会计 report revision 完全未改。密封历史 fixture、原 hash、当前 gold FNV 锚均未改。

## 改动范围

- `packages/engine/tests/config.rs`：完整保留金额数值断言，wire 期望改为整数分字符串；新增两个金额字段数字格式拒绝测试。
- `packages/engine/tests/auction.rs`：indicative_price JSON 期望改为分字符串。
- `packages/engine/src/session/persistence/saved_runtime_tests.rs`：合法 envelope 费用 fixture 改为分字符串，并验证未注入派生字段时可解码。
- `apps/server/tests/api_contract.rs`、`apps/server/tests/ws.rs`：初始化 fixture 使用 Money 字符串；WS 新增 Fixed 数字拒绝，保留旧裸价格拒绝和当前字符串受理断言。
- `apps/server/tests/actor.rs`：player-private baseline 的 cash JSON 期望改为分字符串，不扩大 NPC 账户可见范围。

另按主协调请求，通过真实 Rust TS export 生成 `apps/web/src/types/generated/Money.ts`，不是手写生成类型。export 同时产生 HotParams、SaveSlot 的既有 Rust 注释同步，交主协调统筹。

## 短测记录

编译使用 `CARGO_BUILD_JOBS=16`，仅 engine lib/config/auction 与 server api_contract/actor/ws 定向 test binary 的 no-run 构建，不是完整回归。执行 binary 每条使用 `timeout -k 1s 10s`，Rust `--test-threads=16`；API 与 actor 代表性 case 并行执行。

- config 34 case：通过，包括新增数字 Money 拒绝。
- auction `auction_event_json_matches_frontend_contract`：1 case 通过。
- engine `runtime_envelope_rejects_injected_derived_fields`：1 case 通过。
- server `new_session_returns_200_with_id`：1 case 通过，0.20 秒。
- server `public_baseline_serializes_only_the_player_account`：1 case 通过。
- WS `gateway_reports_malformed_commands_and_queues_writes_explicitly`：1 case 通过，0.20 秒；沙箱内首次无法绑定 loopback，随后使用批准的本地权限执行重编后的当前测试。
- `session::export_bindings_saveslot`：1 case 通过，显式设置 TS_RS_EXPORT_DIR 为当前仓库 generated 路径、TS_RS_LARGE_INT=number；Money 由 ts-rs 导出 string。
- 六个测试源码文件 `git diff --check`：通过。

scripts fixture 子任务完整阅读 `scripts/smoke-deployment.test.mjs`、`scripts/simulation/verify-simulation-artifacts.test.mjs` 与 `scripts/simulation/escrow-verification-contracts.test.mjs`，未发现需迁移的 numeric Money wire fixture，未改文件亦未执行测试。Resource 聚合诊断已有十进制字符串且可能超过 i64，不收窄为 Money。

本记录不宣称完整回归、全部 server/desktop 宿主测试或独立复核已完成；独立 diff 审查由主协调另行安排。
