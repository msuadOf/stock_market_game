# Web Host／Save Schema 检查点独立复核

## 范围

按 `HEAD..worktree` 检查以下完整非 generated manifest；包含 tracked 修改、删除及 untracked source/test。未检查 generated 类型文件，也未改实现、Cargo 或索引。

### Manifest

```text
apps/web/src/host/company-query-coordinator.manual.ts
apps/web/src/host/company-query-coordinator.test.ts
apps/web/src/host/company-query-coordinator.ts
apps/web/src/host/current-minute-history.test.ts
apps/web/src/host/current-minute-history.ts
apps/web/src/host/engine-host.ts
apps/web/src/host/event-buffer.test.ts
apps/web/src/host/initial-allocation-transport.test.ts
apps/web/src/host/intraday-average.test.ts
apps/web/src/host/intraday-average.ts
apps/web/src/host/market-history.test.ts
apps/web/src/host/market-history.ts
apps/web/src/host/npc-decision-trace.test.ts
apps/web/src/host/npc-decision-trace.ts
apps/web/src/host/personal-trade-history.test.ts
apps/web/src/host/personal-trade-history.ts
apps/web/src/host/player-working-orders.test.ts
apps/web/src/host/player-working-orders.ts
apps/web/src/host/protocol-effects.test.ts
apps/web/src/host/protocol-owner-removals.test.ts
apps/web/src/host/protocol-parse.test.ts
apps/web/src/host/protocol-public-events.test.ts
apps/web/src/host/protocol-reducer.test.ts
apps/web/src/host/protocol-runtime-store.test.ts
apps/web/src/host/protocol-test-fixtures.ts
apps/web/src/host/protocol/canonical.ts
apps/web/src/host/protocol/effects.ts
apps/web/src/host/protocol/guards.ts
apps/web/src/host/protocol/normalize.ts
apps/web/src/host/protocol/parse.ts
apps/web/src/host/protocol/reduce.ts
apps/web/src/host/protocol/runtime-delta.test.ts
apps/web/src/host/protocol/runtime-delta.ts
apps/web/src/host/protocol/types.ts
apps/web/src/host/protocol/validate.ts
apps/web/src/host/protocol/wire-values.ts
apps/web/src/host/public-report-normalize.test.ts
apps/web/src/host/public-report-normalize.ts
apps/web/src/host/remote-auth.test.ts
apps/web/src/host/remote-auth.ts
apps/web/src/host/remote-company-reports.test.ts
apps/web/src/host/remote-host-token.test.ts
apps/web/src/host/remote-host.test.ts
apps/web/src/host/remote-host.ts
apps/web/src/host/remote-lifecycle.test.ts
apps/web/src/host/remote-market-context.ts
apps/web/src/host/remote-startup.test.ts
apps/web/src/host/remote-state-contract.test.ts
apps/web/src/host/remote-test-context.ts
apps/web/src/host/report-correction-recovery.test.ts
apps/web/src/host/report-correction-transports.test.ts
apps/web/src/host/report-corrections.test.ts
apps/web/src/host/report-corrections.ts
apps/web/src/host/runtime-snapshot-policy.test.ts
apps/web/src/host/serde-account-id-contract.test.ts
apps/web/src/host/serde-normalize.test.ts
apps/web/src/host/serde-normalize.ts
apps/web/src/host/startup-policy.ts
apps/web/src/host/tauri-archive-generation.test.ts
apps/web/src/host/tauri-host.test.ts
apps/web/src/host/tauri-host.ts
apps/web/src/host/tauri-initialization.test.ts
apps/web/src/host/wasm-worker-ownership.test.ts
apps/web/src/host/wasm-worker.ts
apps/web/src/host/worker-host.test.ts
apps/web/src/host/worker-host.ts
apps/web/src/host/worker-request.test.ts
apps/web/src/save/account-id-contract.test.ts
apps/web/src/save/archive-store.test.ts
apps/web/src/save/archive-store.ts
apps/web/src/save/company-books-schema.test.ts
apps/web/src/save/company-contracts.test.ts
apps/web/src/save/company-schema.test.ts
apps/web/src/save/company-slice-test-fixture.ts
apps/web/src/save/complete-save-schema.test.ts
apps/web/src/save/current-save-fixture.ts
apps/web/src/save/day-end-archive.test.ts
apps/web/src/save/day-end-candidate.ts
apps/web/src/save/fixtures/current-closed-day-save.json
apps/web/src/save/fixtures/current-company-slice.json
apps/web/src/save/fixtures/current-schema-save.json
apps/web/src/save/indexed-db-save-repository.test.ts (deleted)
apps/web/src/save/indexed-db-save-repository.ts (deleted)
apps/web/src/save/indexeddb-save-repository.ts (untracked replacement)
apps/web/src/save/initial-save-source.test.ts
apps/web/src/save/market-memberships-schema.test.ts
apps/web/src/save/personal-schema.test.ts
apps/web/src/save/retained-history-day-end.test.ts
apps/web/src/save/runtime-strategy-position.test.ts
apps/web/src/save/save-schema-contract.test.ts
apps/web/src/save/schema/company/accounting/amount.ts
apps/web/src/save/schema/company/accounting/journal.ts
apps/web/src/save/schema/company/annual-growth.test.ts
apps/web/src/save/schema/company/annual-growth.ts
apps/web/src/save/schema/company/books/bank-loan-source-schema.test.ts
apps/web/src/save/schema/company/books/bank-loan-source-test-fixture.ts
apps/web/src/save/schema/company/books/bank.ts
apps/web/src/save/schema/company/books/income-tax-owner.test.ts
apps/web/src/save/schema/company/books/income-tax.ts
apps/web/src/save/schema/company/books/industrial.ts
apps/web/src/save/schema/company/books/insurance.ts
apps/web/src/save/schema/company/books/real-estate.ts
apps/web/src/save/schema/company/books/tax-owner-test-fixture.ts
apps/web/src/save/schema/company/period-generation.test.ts
apps/web/src/save/schema/company/period-generation.ts
apps/web/src/save/schema/company/publication-source.test.ts
apps/web/src/save/schema/company/report-corrections.test.ts
apps/web/src/save/schema/company/report-corrections.ts
apps/web/src/save/schema/company/reports.ts
apps/web/src/save/schema/company/simple-finance-config.ts
apps/web/src/save/schema/company/simple-finance.test.ts
apps/web/src/save/schema/company/simple-finance.ts
apps/web/src/save/schema/company/simple-values.ts
apps/web/src/save/schema/company/system-config.ts
apps/web/src/save/schema/company/system.test.ts
apps/web/src/save/schema/company/system.ts
apps/web/src/save/schema/market-memberships.ts
apps/web/src/save/schema/market.ts
apps/web/src/save/schema/money-wire.test.ts
apps/web/src/save/schema/monthly-bindings-contract.test.ts
apps/web/src/save/schema/orders.ts
apps/web/src/save/schema/personal-trade-confirmations.test.ts
apps/web/src/save/schema/personal/beliefs.test.ts
apps/web/src/save/schema/personal/beliefs.ts
apps/web/src/save/schema/personal/common.ts
apps/web/src/save/schema/personal/information.ts
apps/web/src/save/schema/personal/plans.ts
apps/web/src/save/schema/primitives.ts
apps/web/src/save/schema/report-frequency.test.ts
apps/web/src/save/schema/report-frequency.ts
apps/web/src/save/schema/retained-history.test.ts
apps/web/src/save/schema/retained-history.ts
apps/web/src/save/schema/root.ts
apps/web/src/save/schema/runtime-state.ts
apps/web/src/save/schema/save-snapshot.ts
apps/web/src/save/session-replacement.ts
apps/web/src/types/engine.ts
```

## 发现（Must fix）

1. **u64 股本跨 JS 边界仍用 number。** [spec.ts](../../apps/web/src/save/schema/company/accounting/spec.ts) 将 `CompanySpec.issued_shares` 解析为 `integer`，而 Engine 定义为 `u64`；同档案股票 `total_shares` 已按十进制字符串处理。超过 `Number.MAX_SAFE_INTEGER` 时，WASM 的 `normalizeSerdeMaps` 会因 bigint 转 number 不安全而拒绝保存；JSON/远程解析则无法无损表示该股本，`validateCompanySystemSession` 对股本匹配的比较也只能接收已精度受限值。这让大股本公司出现“行情总股本可表达、对应发行人不能保存/恢复”的跨层契约漂移。应把发行人 `issued_shares` 以规范 u64 字符串贯穿 SaveSlot parser、Host serde 规范化和交叉校验，并补大于 JS 安全整数的完整存档往返用例。

2. **通用 u64 parser 接受非规范十进制。** [primitives.ts](../../apps/web/src/save/schema/primitives.ts) 的 `decimal` 只校验 `^\\d+$` 与上界，因而接受 `"00"`、`"00042"`；新 AccountId、远程 generation/seed 与若干专用 RNG parser 已要求规范形式，而其余 seed、receipt、cursor、股数等通过 `decimal` 仍接受非规范表示。这与当前完整 SaveSlot、无兼容的严格契约及 `canonicalU64` 不一致，且重新输出会继续保存原非规范值。统一规范 u64 校验或将需允许前导零的域明确拆为不同语义，并覆盖具体字段拒绝用例。

## 审查结论

- **A 股语义：** 本范围内新增 Host 数据校验将金额明确保持为分单位 `Money` 字符串，将成交股数和成交金额独立校验，并区分公开成交、个人成交事实、自然日休市／无成交／未结束；未发现其改变沪深交易规则的迹象。公司 `issued_shares` 与股票 `total_shares` 必须精确匹配的语义正确，但上述 JS 精度问题会使该契约无法覆盖整个 u64 取值域。
- **必要性／范围：** Host generation 隔离、账户字符串、报告来源、严格完整存档和 IndexedDB 日终介质都与此检查点的 Host/Save 契约有关；实现范围集中。旧 IndexedDB repository 测试与实现的替换符合新接口安排，需保留对当前事务生命周期和 generation 隔离的覆盖。
- **边界覆盖：** 用户提供的作者验证记录包括 fresh release 66 fixture、restore deep equality、三帧真实 NPC acceptance、Web save 15 与独立 operations parser 22 通过。本复核没有自行运行测试，也不据此宣称 full runtime 已验收。上述两项需修复后加严格断言，并按项目门禁再次由未实施者复核。
