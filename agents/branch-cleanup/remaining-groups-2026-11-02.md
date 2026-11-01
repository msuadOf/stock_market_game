# 剩余 branch 的待核销范围

冻结对照 `6fe53f65`，盘点时基线仍在前进。以下是下一步，不代表这些 branch 永久保留；“尚未核对”也不代表功能缺失。保留工作树、stash 和 WIP，任何转换前须重新检查 HEAD、状态及活动使用。

## 可考虑转 tag 的历史证据

- `test/company-b4e-actor-e2-base-2026-10-28`：约 1299 项自然日信息首红、receipt、manifest 等来源证据不在冻结基线。需确认测试行为已有覆盖；若差异仅为历史证据，可转 tag 保存原 tip。
- `feat/web-correction-input-parser`：39 项真实 serde 和红绿记录缺于基线；`feat/web-proposal-input-parser` 缺实现记录。需先核对当前解析合同及断言；仅证据差异时可转 tag。
- `feat/tax-admin-projection`：13 项官方税务原件、检索收据、caller 合同与独审记录缺于基线，适合在确认没有待实现合同后转 tag；官方来源仍应能追溯。
- `feat/wasm-w1-combined-build-monitor`：基线没有历史 build-monitor 工具及红绿日志。判断它是否仍承担正式门禁；若仅为历史证据可转 tag。
- `codex/clippy-cfg-args-leaf-rework-20261030`、`codex/clippy-cfg-npc-tests-20261030`、`codex/clippy-cfg-settlement-rework-20261030`：基线缺 handoff 或 implementation 记录，源码有后续演进。核对相关生产和测试改动后，可用 tag 保存历史记录。
- `fix/finish-120-groups2`、`feat/reintegration-b4c-dayend-chain`：缺性能探针、缺陷台账或验收收据，但还含较多非等价行为提交。先核销行为，再判断能否转 tag。

## 需要进一步逐行为对照的候选

- `feat/wasm-w2-primitives-2026-10-28`：原 23 项命名测试中抽查的 FIFO、shutdown sentinel、quiet clock regression 未找到对应测试。应定位迁移后的等价断言；若确实缺失，评估迁移测试。
- `feat/wasm-w1-combined-cargo-inputs`：原 2 GiB 与测量字段已演进为 3 GiB 和 `liveUpperBoundBytes`。核对现行容量合同、构建输入测试及证据，不可直接将两个策略视为等价。
- `codex/wasm-allocation-observer-20261029`、`codex/wasm-production-stop-20261029`、`feat/wasm-restore-topology-2026-10-28`、`feat/wasm-w1-spawn-ui`：已找到部分对应生产行为或测试，仍须检查分支早期 shutdown、恢复、spawn 及证据；若全部覆盖则原历史可转 tag。
- `feat/company-b4e-correction-lifecycle-2026-10-28`、`feat/company-b4e-repurchase-common-2026-10-28`、`feat/company-b4e-repurchase-route-2026-10-28`、`feat/approval-c-common-93-2026-10-28`：逐项对照更正调用、可信批准、行政镜像与自然日边界及恢复断言；缺口经确认后迁移实现或测试。
- `feat/registry-physical-day-phase`、`feat/admin-funds-cost-p1`：分别核对 physical day 相位/恢复和付款、退款、回购信用、资金与信息共同回滚的边界；尚不能断定存在缺口。
- `codex/clippy-idioms-sessionextra-20261030`：约 30 个文件已演进，需逐 hunk 对照后再决定历史引用处置。
- `test/company-b4e-actor-recovery-2026-10-28`：基线含两宿主同名测试且有扩展，但尚未逐断言确认原测试覆盖。
- `integrate/product-union-2026-10-28`：按自然日日终、回购 Calendar、Closed retained authority、WASM 收尾及测试逐项映射，不能由提交标题推断吸收。

## 工作树 WIP、活跃任务或保全 stash

- 活跃或保护：`main`、`codex/branch-convergence-2026-11-01`、`fix/multi-group-roster-offset`、`codex/wasm-company-parity`、`codex/wasm-sync-parity-20261102`、`codex/r58-supervisor-audit`、本记录分支 `codex/remaining-ref-audit-20261102`。
- 公司与回购：`codex/company-write-impl-2026-10-29`、`feat/company-r4-production`、`fix/web-correction-restatement-2026-10-28`、`test/company-write-host-red-2026-10-28`、`integrate/company-write-web-three-transports-2026-10-28` 均有未提交内容或冲突；`feat/b4c-rights-offering-r3b` 和 `feat/company-b4e-commands` 有对应 stash。先保全并识别 WIP 归属，再核销各功能和证据。
- 日历和行政：`feat/engine-mixed-calendar-c2c`、`feat/engine-mixed-calendar-main-c2c-20261028`、`feat/civil-external-owner-lease-2026-10-28`、`codex/closed-day-protocol-retained-20261030`、`codex/p1-group-consumer-2026-10-29` 有工作记录或修改；前者还有关联 stash。`codex/last-integration38-skeleton-20261030` 有未跟踪 diff，基线记录尚未包含原 green 收据。
- WASM：`codex/wasm-document-owner-20261029`、`codex/wasm-restore`、`feat/wasm-w1-build-gate`、`feat/wasm-w3-storage-2026-10-28` 有修改或未跟踪内容；restore 仍有两项未等价优化，须核对 PreparedRestore 与实际恢复性能。
- Clippy 与性能：`codex/clippy-cfg-candidate2-approved-20261030`、`codex/clippy-idioms-company-20261030`、`codex/runtime-sync-progress-cost-20261030` 有未跟踪证据，须先确认来源及行为。
- `codex/r4-tax-production-2026-10-29`：须核对当前工作树及其税务 native 支持边界，再决定归档；不得把尚未核销误认为实现缺失。

以上保留原因是现有核对边界。无产品修改、无产品测试；本次不声称剩余功能缺陷已被证实。
