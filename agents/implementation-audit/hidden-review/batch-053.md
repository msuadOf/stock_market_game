# 隐藏审计扫描 batch 53

## 范围与来源核验

依据 scan-plan `source_baseline=43b1aa5`，逐源从源根连续读取至 EOF；三份均无截断，SHA-256、行数及 aliases 与计划吻合：

| 源文件 | 行数/EOF | SHA-256 | aliases | 章节状态 |
| --- | ---: | --- | ---: | --- |
| `agents/oop-refactor-audit/exhaustive/reviews/engine-company-01.md` | 32，EOF | `83ff36ebf962140bf76f82332f9f2365c8c7e4694c5e2adcfd9f09f8d702a777` | 1 | 全文：三项旧阻断、领域与范围门禁、遗留风险及限制 |
| `agents/oop-refactor-audit/exhaustive/reviews/engine-company-02-projection-first-recheck.md` | 25，EOF | `b830f5e59cbd4c09f8b008cfed54ae9aa45ada71f618e7c849b9f82d5932b383` | 1 | 全文：措辞复核未通过、差异证据、三项门禁 |
| `agents/oop-refactor-audit/exhaustive/reviews/engine-company-02.md` | 25，EOF | `80f1ad82f6e1344b0b2659afe4e0ecf344bce9c5ff1f35a2a98dc1b3f7867291` | 1 | 全文：最终措辞复核通过、证据链与三项门禁 |

核对当前 caller 的 `AGENTS.md`、`docs/principles.md`、`docs/open-questions.md`；本批公司经营语义对应 ADR-0016 与 `docs/company-accounting.md`。caller 最新裁定以 `agents/implementation-audit/exhaustive-review/resolution.md`、`agents/implementation-audit/reaudit-engine.md` 及 `coverage-index.md` 为准。没有发现这三份历史复核引入或改变 A 股交易制度；会计单位与交易单位不可混为一谈。

## 追踪与裁定

- `engine-company-01.md` 对采购后子账溢出的表述仍有源码路径支持：`packages/engine/src/company/industrial/purchasing.rs:45-115` 先算金额、执行 `post_with_commit`（约 :103），再调用 `inventory_mut().receipt`（:115）；运行时工商调用入口为 `packages/engine/src/company/operations/industrial.rs:146`。`write_off_receivable` 同样先 `post_with_commit` 再 `receivables_mut().write_off`，见 `packages/engine/src/company/industrial/sales.rs:214-249`。本次静态核对没有执行或复现错误。
- 该历史复核自己已明示：采购溢出行为未修复、caller 缺少直接断言；D01/D02/D03 是既存风险而非修复事实。当前 caller `reaudit-engine.md` 的 G35 是工商期末经营闭环缺少折旧、所得税及债务支付等；它没有把此原子性边界宣称已修复。现行 resolution 还把零额合法折旧并入 G35。没有证据表明此处有旧的“已修复/已核销”结论需要撤销。
- `engine-company-02-projection-first-recheck.md` 的失败项是模块审计文档中“类型保证的派生投影”措辞不准确；随后 `engine-company-02.md` 记载只剩首段替换且措辞闭合。当前隐藏任务仅要求审查已批准承诺遗漏/旧核销错误，未发现该历史文案偏差对应一个仍遗漏的已批准产品承诺；不登记新产品候选。
- G 关联：无新候选。G35 仍按 caller 既有缺口保留；不以本批旧风险重复编号或扩张 G。没有真实运行证据，故不报告运行时复现。来源中的缺覆盖声明也未被当作测试通过。

## 结论

本批没有识别出已批准承诺遗漏或旧核销错误。历史记录提及的先写后溢出属于未修复、且未被本轮或 caller 声称核销的风险；它不因被重新发现而自动变为本任务范围内的新 G。未改产品代码，未运行测试/回归。
