# 隐藏扫描批次 078

- 基线：`43b1aa5`；source root：`/data1/baiyifan/workplace/stock_market_game`；caller：`/data1/baiyifan/workplace/stock_market_game/.worktree/implementation-reaudit`。
- 规范：全文读取 caller `AGENTS.md`、`docs/principles.md`；核对相关现行 ADR-0017、ADR-0018、ADR-0027、ADR-0028、`docs/open-questions.md`、`docs/testing.md`，以及当前总账、`reaudit-engine.md`、`reaudit-tools.md`、`candidate-checks.md`。历史 agent 指令仅作为来源，不作为现行规范。
- 方法：三份 source 连续读取到 EOF；实测行数和 SHA-256 均与计划一致，每源 aliases=1。无截断补读需求。未运行测试、构建、Git 或交易规则核验；仅写本批 Markdown/JSON。

## 来源全文与判断

| 来源 | EOF / 行数 / SHA-256 / aliases | 全文结构与判断 |
|---|---|---|
| `agents/oop-refactor-audit/exhaustive/reviews/root-relationships.md` | EOF；16 行；`87fc20cb05feb0bc1b52e882d697af932690551045a640bc737d3baf6dee8646`；1 | 含前轮记录、本轮复核、结论。该文明确其范围仅为文档证据措辞，不是源码或交易规则复核。没有可据以新增产品 G 的承诺遗漏；它自身也说明不可由 hash/文件数推出源码覆盖。 |
| `agents/oop-refactor-audit/exhaustive/reviews/session-strategy-document-binding-final.md` | EOF；19 行；`9aa67778cb4b0a3828806eb2b0c00d00cec05c99b1fa3b59f9a418ead7545a68`；1 | 含范围与方法、ES04/ES07/ST01 三批绑定表、结论。只是 items/modules 与既有复核记录的哈希和事实绑定，不重做源码复核；旧 G16、D01/D02 等仍按 caller 现行审计分类，不能由这次文档 binding 核销。 |
| `agents/oop-refactor-audit/exhaustive/reviews/tooling-01.md` | EOF；65 行；`07f43486d3e4477ab5111e06d5420917717a17730412af22c5589cb78fc4e950`；1 | 含多轮 review、工具/fixture/build 跨文件发现、处置及最终 binding。既有 G21/G26/G39 与 Q05 的限定和 caller `reaudit-tools.md` 基本一致；ADR-0028 明确发布仅构建，移除发布测试不是遗漏。早期 D02 的整树退出目标已由后续轮次收窄为“有 containment 确认才清理；否则保留并报告未确认”，未将目标冒充当前保证。 |

## G/Q、caller 证据与候选

- Caller 的 `reaudit-tools.md` 保留 G21 输入投影、G26 手动 CI lint warning、G39 自由调度输出比较及 Q05 持续发现入口状态；G27 仍按 ADR-0028 核销。总账 `implementation-audit-2026-10-02.md` 同样将 G39 限定为工具门禁错误比较自由调度结果，不将已接受的并发受理差异称为生产重放缺陷。来源没有证据支持修改这些状态。
- Tooling 来源第五轮特别指出 deadline helper 的两条超时文案声称“process tree was terminated”，实际仅发出终止请求；第六轮只绑定 items/module/review 的关系措辞，没有重审这一代码文案。当前 caller `scripts/run-with-deadline.mjs:80-86,118-127` 仍在 execution timeout 和 hard timeout 错误消息中声称树已终止；`terminateTree` 在 Windows 是 `taskkill` 异步 `unref()`，POSIX 是发送 SIGKILL，且 hard timer 可在直接 child `close` 前拒绝。`docs/testing.md:91-100` 要求诚实的有界进程树终止/收敛与清理状态。当前 `reaudit-tools.md` 的 G/Q 列表没有记下此可见错误文案。**候选（待总账分配，不自行新增编号）：** 将两条错误改为准确描述终止请求/未确认状态，避免将信号请求或直接 child close 冒充整树终止。仅据静态代码可确认报告语义不准确；未复现实际超时或运行失败，不声称每次 timeout 均有后代存活，也不把此项扩大成新的进程隔离设计。
- 当前引用的其余代码证据：`scripts/build-targets.mjs:12-15,63-107` 定义 300000ms build deadline 与 jobs；`docs/decisions/0028-tagged-release-and-static-pages.md:15-35,84-93` 决定发布/手动入口边界。Source tooling 关于 BuildRun/ArtifactPublisher 的范围与该决定兼容。未看到交易制度、资金或股份语义变化。
- 反证/限制：caller 已在 G39 说明 K7、after/sensitivity 证据约束，且不将自由并发的不同输出当生产不变量；`docs/testing.md` 对 timeout 包装器有明确条款，但当前两条用户可见错误不准确这一点未在 caller 工具复核中登记。其余提到的未测边界均没有运行证据，按要求不升级为运行故障。策略文档 binding 没有提供当前代码差异证据，不能仅靠历史复核中的 `retain` 或 hash 推导已实现。

## 结论

三源完整性核验通过。未发现它们足以核销现有 G/Q 的新证据；发现一个范围狭窄、可从当前代码直接证实的 deadline 错误文案候选，建议由主审计决定归入既有工具缺口还是独立记录。静态审查，不代表测试、进程树终止或清理已验证通过；不涉及大 A 规则改动。
