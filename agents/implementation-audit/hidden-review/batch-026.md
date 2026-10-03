# 批次026 独立复核

审查基线：`43b1aa5f25226c72976ca172d32f4a8eeb2272ad`。只读核对源码、文档与现行调用者；未执行 Git 写操作、测试、构建或交易规则查询。OOP 抽取本身不作产品缺陷修复认定。

## 来源读取与完整性

| 来源 | 计划 SHA-256 / 行数 | 实读 | EOF | 章节族状态 |
|---|---|---|---|---|
| `agents/oop-refactor-audit/completeness-2026-10-03/session/parts/15.md` | `33d671ced72d14ad9a4e9bbca9d76b205c3e3961ce83c5dacbcbe8385e5e54a0` / 11 | 11 行连续通读 | 到达 EOF | engine-strategy-03 中 `value.rs` retain、`zi_noise.rs` A01；原结论再证。A01 的 `strategy_data` 当前已存在，属当前代码，不把抽取记作缺陷修复。 |
| `agents/oop-refactor-audit/completeness-2026-10-03/session/parts/16.md` | `d4086a936f4ecf56f218c05d4716fd00f0d1f574eaba79703d938f6a2588adfd` / 7 | 7 行连续通读 | 到达 EOF | engine-session-06 / ProtocolSession 保留；原结论再证。重复 checkpoint 状态组合仅为低收益候选，且原记录已提出，不升级为新候选。 |
| `agents/oop-refactor-audit/completeness-2026-10-03/session/parts/17.md` | `215af163d3777c6c4bf01a7be2f980e104d98c34682a77e5445e7abc72785a19` / 21 | 21 行连续通读 | 到达 EOF | engine-session-07 commit/delta/facts/serde/player_orders/replay/reconciliation/self_views；原 disposition 再证，没有新增 OOP 动作。 |

## 当前代码与 caller 对照

- `BeliefInstitutionStrategy` 仍只承载机构身份、观察节奏和个体参数；个人估值/方向/计划留在会话决策链。当前定义及空 `decide`、不更新空决策报价见 `packages/engine/src/strategy/value.rs:37`、`:45`、`:149`、`:160`。没有证据支持拆目标价对象；ADR-0026 把个体经历与暂停记忆界定为账户决策状态，支持此边界。
- ZiNoise 的字段投影已由 `ZiNoiseStrategy::strategy_data` 集中在 `packages/engine/src/strategy/zi_noise.rs:40`，三个调用入口复用它（`:140`、`:178`、`:225`）；转换意图的纯函数仍在 `:239`。旧记录提出的窄 A01 已在基线实现，不代表策略语义或 defect 状态改变。ADR-0006、ADR-0021、ADR-0026 支持参数/决策边界；本批不主张新的交易规则。
- `ProtocolSession` / `ProtocolCheckpoint` 的回滚字段及 checkpoint/rollback 仍在 `packages/engine/src/session/protocol/civil/session.rs:59`、`:67`、`:73`、`:79`；帧提交和 delta 仍分属 `commit.rs:7`、`delta.rs:182`。真实宿主 caller 为 server actor（`apps/server/src/actor.rs:1010`、`:1041`、`:1167`）及 desktop actor（`apps/desktop/src-tauri/src/actor.rs:676`、`:777`、`:911`）；WASM registry 仍持有 ProtocolSession（`apps/web-wasm/src/lib.rs:51`、`:54`）。ProtocolSession 是发布事务 owner，状态组合不应迁移其事务责任。rollback 对已成功发布 runtime/candidate 的覆盖是可核对边界；源记录已标出部分独立断言缺口，后续改变回滚状态组前需补锚点。
- `commit.rs` 的投影仍由 GameSession 提交后结果驱动；`delta.rs:157` 提供 baseline 投影，`facts.rs:41` 做单次 identity 转换；`player_orders.rs:29` 从订单簿投影玩家工作单；`replay.rs:10`、`:25` 的 ReplayGuard 独占游标与 accepted map；`self_views.rs:8` 并行读后排序归并。它们分别保留既有 authority，创建共享索引/管理器会新添同步权威。

## G/Q 与取代检查

对照 `agents/implementation-audit/implementation-audit-2026-10-02.md` 的 G01–G68/Q 总账、`coverage-index.md`、OOP `action-index.md`，并读 `docs/decisions/0019-draft-market-scope-and-capacity.md`、ADR-0021、ADR-0025、ADR-0026、ADR-0027、ADR-0028。G/Q 均不因这些 OOP 结论而核销；尤其 ProtocolSession 的结构/回滚 owner 不能证明发布故障、宿主调用或持久化问题已修复。没有发现这些来源与更晚 ADR 明确取代的 disposition。对本批不相关的交易制度不作新主张。

## 候选与结论

旧结论再证：三篇所述 retain/support 及状态所有权理由与当前调用链一致；唯一需标明的基线差异是 ZiNoise A01 已落地。未发现新候选。针对 ProtocolCheckpoint 字段镜像的 `ProtocolState` 组合，旧来源已经明列且收益有限；可能减少字段漏同步，但当前字段显式、无证据表明它修复 G/Q 问题，暂不推荐新增。所有交易语义仍由现有 A 股账户、订单、结算及 protocol DTO 边界表达；本次没有改动产品代码。
