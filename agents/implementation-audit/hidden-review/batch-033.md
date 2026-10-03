# 隐藏扫描批次 033

## 范围与读取证据

- 按 `scan-plan.json` 的 owner=3 扫描三份来源；source root 为 `/data1/baiyifan/workplace/stock_market_game`，当前 caller 为 `.worktree/implementation-reaudit`，历史基线标签为 `43b1aa5`。
- 三份来源均连续全文读至 EOF；实测行数、SHA-256 与 plan 一致。逐章状态及来源完整性见下表和配套 `batch-033.json`。
- 当前 caller 的 `AGENTS.md`、`docs/principles.md`、`docs/testing.md` 已读取。与工具期限/Pages发布有关的最新 ADR 为 `docs/decisions/0028-tagged-release-and-static-pages.md`；它规定发布链调整，不免除测试/清理期限，也没有批准本批 OOP 候选。`docs/decisions/0019-draft-market-scope-and-capacity.md` 与这三份工具/所有权材料无关。早期 agent instructions 和审计文档中的旧基线、旧政策仅作历史证据，不作为当前授权。

| 来源 | 全文/EOF | 章节状态 | 当前 caller 对照 |
|---|---|---|---|
| `agents/oop-refactor-audit/exhaustive/areas/tooling.md` | 完整；38 行；SHA 匹配 | 区域结论、子模块对象归属、建议迁移顺序、分类/复核限制均已读到末行。它是“候选设计，未实施”的汇总，明确区分对象所有权与行为缺陷。 | 多项风险仍可在真实路径定位：CDP pending 未 settle `scripts/performance/market-ui-report.mjs:120-154`；Windows taskkill 仅等待 `exit` 且 `unref` / 异步返回 `scripts/run-with-deadline.mjs:10-18`；Pages 静态读取仅做词法 containment 后直接 `readFile`，未阻止 root 内 symlink `scripts/smoke-pages.mjs:15-25`。Build supervisor 已有显式阶段/安全清理，不能把旧描述直接当作当前缺陷。 |
| `agents/oop-refactor-audit/exhaustive/managers/domain.md` | 完整；20 行；SHA 匹配 | 管理范围、最终分类、审查边界、root 回看、限制均已读到末行。关于 Account/Company 的结论与本次工具对象抽取无关。 | 无需将其历史 domain 动作映射为新工具/OOP动作；不得把旧 review 的“通过”解释成工具运行时行为已验收。 |
| `agents/oop-refactor-audit/exhaustive/managers/execution.md` | 完整；15 行；SHA 匹配 | 执行域、12批/111文件、六个动作、未决事项及历史审计限制均已读到末行。明确说明仅改调查材料、无源码/测试/Git。 | 执行域旧记录不能核销当前 G 缺口；其列举的 deadline、失败隔离等只是边界说明。本批没有发现遗漏的已批准实现承诺。 |

## 调用链、缺口关联与反证

- **G61 仍有有效关联（已有缺口，不是本批新发现）**：`MarketUiReportRun` 已经持有 UI 性能运行资源（`scripts/performance/market-ui-report.mjs:234-245`），但 `CdpClient.close()` 只关闭 socket，pending requests 没有断连结算（同文件 `:120-154`）；`stopProcess()` 的 Windows 路径在 killer `exit` 后返回，POSIX 只发 SIGTERM（`:224-232`）。这与当前总账 `agents/implementation-audit/implementation-audit-2026-10-02.md:129` 的 G61 失败收尾/整体 deadline 范围一致，不能因 owner 类已经实现而核销。
- **G62 仍有有效关联（已有缺口，不是本批新发现）**：`runBoundedCommand` POSIX child 以 `detached` 启动（`scripts/run-with-deadline.mjs:69-89`），终止只对直接 child PGID 发 SIGKILL，Windows 则 `taskkill.unref()` 不等待树终止回执（`:10-24`）。其工具层约束见当前总账 `...implementation-audit-2026-10-02.md:130`。本批区域文档准确提醒 supervisor 与 worker 所有权不同；不能由 `finally`/owner 聚合取代外层 supervisor。
- **G63 仍在现行总账**：总账 `...implementation-audit-2026-10-02.md:131` 记录普通 Rust case 缺独立 10 秒 deadline。三个来源都没有提供相反实现证据，也不应从拥有资源对象或完成批次汇总推导已解决。
- **sampler 异常清理仍需关联 G61**：`ProcessSampleRun.start()` 先等待 child close 再 await sampler（`scripts/simulation/escrow-performance-harness.mjs:383-400`）；若 probe 提前拒绝，没有在等待 close 前终止 owned child 的分支。当前 source 文档和旧审计对这一边界的描述一致，但其是否属于同一 G61 修复范围应遵从现行总账，不在本批扩张为独立 G 编号。
- Pages symlink 指向 root 外的风险仍由 `smoke-pages.mjs:20-25` 的词法 containment + `readFile` 可达；这是源码路径核对，不据此凭空创建额外的已批准 OOP 工作项。当前总账是否要求其修复应按对应正式承诺另行评估。
- **无新增候选**：tooling 汇总中的 `BuildRun`/Publisher、baseline、matrix 等均明示为尚未实施的可选设计；其余两份是历史管理交接。没有证据表明这些提案已成为批准承诺，故不把其未实现认定为遗漏。实际存在的 supervisor、CDP client、ProcessSampleRun 等 owner 也不证明相邻行为缺陷已关闭。
- 本批不涉及交易语义实现、规则或 A 股法源；未观察到领域概念、单位、存档/API 契约漂移。未运行测试、构建或 Git，也不将缺少运行证据本身当成问题。

## 结论

三份来源与调用边界描述未显示已批准 OOP 承诺遗漏或旧核销错误。可确认的运行时残余继续按当前 G61/G62/G63 及相邻工具边界跟踪；它们不是本批新建实现或“已修复”结论。三个源的 EOF/hash/行数完整性记录见 `batch-033.json`。
