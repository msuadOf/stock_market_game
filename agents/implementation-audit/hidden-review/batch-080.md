# 隐藏扫描批次 080

- 基线：`43b1aa5f25226c72976ca172d32f4a8eeb2272ad`；source root：`/data1/baiyifan/workplace/stock_market_game`；caller：`/data1/baiyifan/workplace/stock_market_game/.worktree/implementation-reaudit`。
- 范围：仅审 `tooling-05.md`、`tooling-06.md`、`tooling-area-final.md` 三份既有 review。逐份连续读取至 EOF；计划行数、SHA-256 与实测值完全相符，均为 aliases=1。
- 依据：核验基线 caller 的实际函数调用/消费者/状态 owner，并核对 `implementation-audit-2026-10-02.md`、`reaudit-tools.md`、G/Q 主账、`docs/open-questions.md`、`docs/principles.md`、`docs/testing.md` 与相关现行 ADR-0017/0018/0027/0028。未运行测试、构建或交易规则核验；仅写本批 Markdown/JSON。

## 来源与复核

| 来源 | EOF / 行数 / SHA-256 / aliases | 判断 |
|---|---|---|
| `agents/oop-refactor-audit/exhaustive/reviews/tooling-05.md` | EOF；81 行；`9dfb7b463488f43946aa177515c42fafcbb21daddecdf96daf52cb8dd6c7be96`；1 | 含早期完整审查、D01 symlink 缺陷复核、EscrowVerificationRun action 与 D01 分类 delta、最终一致性、metadata 校正。记录的 `validateArtifacts` 风险与基线代码一致：先 `realpath(absolute)`、再 `readFile(canonical)`、之后才检查 output containment；读前未 lstat，内部 symlink 可指向 output 内另一文件，外部目标内容则先被读取再拒绝。明确不能把 D01 说成已修复。需更正其“源码和测试尚未变更 / 候选未实施”的时态：基线 `run-escrow-verification-matrix.mjs` 已有 `EscrowVerificationRun` 类，公开 `runEscrowVerificationMatrix` 通过 `.create(...).execute()` 调用；候选生命周期聚合在基线已经存在。报告对当时文档修订的复核结论仍可作为历史记录，但不能作为基线实现状态。 |
| `agents/oop-refactor-audit/exhaustive/reviews/tooling-06.md` | EOF；45 行；`23896b7b99e608869e97b13d917f3d9ebe587e862cb3105fef580d40890312f2`；1 | 对 smoke-pages 的路径限制判断准确：`path.resolve` 后只有词法 containment，之后 `readFile(filename)` 跟随 symlink；root 下指向 root 外文件的链接可逃逸。提出内部/外部链接与缺失目标测试属于未来修复，不声称现状安全。tooling-06 文件面向 smoke/build/workspace，不涉及 A 股状态。metadata SHA 的历史记录有旧值，但最终记载的 tooling-06 item/module 哈希在区域复核中一致；这三份指定来源本身的完整性以本表哈希为准。 |
| `agents/oop-refactor-audit/exhaustive/reviews/tooling-area-final.md` | EOF；32 行；`1b00d13fd6e82f28655e4dc486c98347210cc794c7558518ac12d1164d6b926b`；1 | 区域整合准确区分候选 action、支持材料与 defect leads；tooling-05-A01、tooling-06 的路径风险与其最终复核材料相符。其所列 87 文件/4 个候选 action 与批次分类仅为历史文档绑定，不能推导候选均未实施；尤其 tooling-05 `EscrowVerificationRun` 已在基线源码中存在。G39 属 K7 输出比较验收边界，不是上述两条 symlink 缺陷的核销依据。 |

## Caller 现状与交叉边界

- **tooling-05 owner/consumer：** 基线 `scripts/simulation/run-escrow-verification-matrix.mjs` 中 `EscrowVerificationRun` owns normalized config、source manifest、request/hash、entries、baseline vector 与 per-slot executor order；CLI `main` 调公开 runner，runner 创建并执行实例。`scripts/simulation/escrow-source-manifest.mjs` 被 performance config、performance harness 与 matrix runner 消费；matrix contracts validator 核验引擎证据。独立 `verify-simulation-artifacts.mjs` 是只读 K7 artifact verifier，不是 matrix runner 的 owner。基线已实现对象聚合，但 `validateArtifacts` 的读前 symlink 风险仍在；两件事不可混为一项。
- **tooling-06 owner/consumer：** `scripts/smoke-pages.mjs` 的 `smokePages` 在本模块内持有 HTTP server/browser/context 生命周期；CLI 入口调用它。`scripts/smoke-deployment.mjs` 的部署子进程由 `launch`/`managed` 管理。`wasm-build.sh` 与 `.bat` 转发给 `frontend-build.mjs`；`workspace-paths.mjs` 被各脚本调用者复用。Pages 本地静态 smoke 的 symlink 风险没有在该路径上看到 containment 保障，且 review 明确缺少相应专用测试。
- **G/Q：** 当前总账把 G39 限定为 K7 在自由调度下对比完整产物/重跑 stdout hash 的不当验收约束；不匹配这里的文件读取路径缺陷，不应据此核销。当前 G/Q 项未找到能准确代表 `validateArtifacts` 或 `smoke-pages` symlink 越界的条目；建议作为未分配工具缺陷交主审计确认，不自行分配编号。`docs/open-questions.md` 没有相关待决问题。ADR-0017 的 cash cents/shares/T+1/envelope 契约只适用于游戏模型；ADR-0018 仍有 proposed 部分；本批工具文档没有改变交易语义。ADR-0027/0028 确认 runtime/build/Pages 范围，不提供 symlink 跟随安全保证。

## 结论

三份 review 均已按计划完整读取且指纹匹配。两项 symlink 读路径风险在基线仍成立；应作为两个不同 I/O 边界追踪，且尚无准确 G/Q 项。tooling-05 review 的历史复核过程准确区分 D01 与 action，但其“未实施”状态被基线 `EscrowVerificationRun` 实际实现推翻，应在总账状态更新时改写为“action 已实现、D01 未修复”。tooling-area-final 的聚合数量可视作当时审计时点结论，不代表基线仍未实施。没有发现需要变更 A 股规则或重新裁决 Q 的依据。此静态审计不表示测试、构建或 symlink 行为已验证。
