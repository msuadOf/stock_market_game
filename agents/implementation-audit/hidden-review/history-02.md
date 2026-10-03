# 历史来源补扫 02（sources 下标 3–5）

基准源码：`43b1aa5f25226c72976ca172d32f4a8eeb2272ad`。三份原文均从计划固定 revision 以 `git show revision:path` 连续读取至 EOF；行数、SHA-256 与计划一致。本文仅登记历史材料中可能影响当前实现审计的内容，不把旧分支或旧计划文字视作 accepted。

## 来源与分类

| 来源 | 分类 | 核对结论 |
|---|---|---|
| `.omo/notepads/desktop-build-matrix/problems.md`（15 行） | 已实现；总账去重 | 此文记载的 NSIS `/CMDHELP`、指定 `lld-link`/`llvm-rc`、MSVC target、Corepack 管理的 pnpm、Windows `ComSpec` 调用和签名说明均为历史修复记录。当前 `scripts/desktop/build-matrix.mjs` 存在对应检查和 caller，`scripts/desktop/build-matrix.test.mjs` 覆盖缺依赖阻断、精确工具名和调用参数。按总账 S03 的工具链覆盖范围去重；未发现新的实现候选。签名要求只关联 Windows 打包选择，不改变 A 股交易语义。 |
| `.omo/notepads/escrow-parallel-engine/decisions.md`（166 行） | 已实现、退役；未来事项已由正式决策/总账承接 | 多处旧事件因果排序与早期 `TickFrame` 顺序约束在文内已明示废止；以 ADR-0017 当前正文和 ADR-0018 后续修订为准。其余实现阶段记载与 ADR-0017/0025 及现行 pipeline、CivilUpdate、SaveSlot v2 实际接线核对，不将中间 Todo 批次授权当成现行实现契约。总账 S06 已覆盖 `.omo` 历史材料；协议、存档与集成边界已在 resolution 的 G/Q/未来状态归类。无新增候选。 |
| `.omo/notepads/escrow-parallel-engine/issues.md`（448 行） | 已实现、退役、未来事项；总账去重 | 文档主体是历史验证限制、修复轨迹和当时的 detached-worktree 阻断记录。旧 `OrderAccepted → AuctionTick`/causal-order 争议已由 ADR-0017 明确取代；P2-P9 接线及存档的阶段性状态不能直接推定在基准源码缺失。对照 43b1aa5 的 `plan_tick`/`commit_tick` caller、pipeline 实现、协议与持久化消费者，并按 resolution 中 G/Q 与未来边界消重。与既有账目相同，不构成新增候选。 |

## 复核结论

- 新候选：无。三项来源内容分别是已落地工具修复、被正式协议取代的中间讨论，或已由总账覆盖的历史实施/验收轨迹。
- 本分片没有提出交易制度改动，也没有把托管账本或历史顺序安排解释为交易所清算规则；A 股语义依据仍以现行 `docs/trading-rules.md` 与已接受 ADR 为准。
- 本分片未运行测试，也未改变代码。该结论只表示这些来源没有带来新的审计候选，不代表相关子系统不存在已登记缺口。
