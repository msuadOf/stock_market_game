# ADR-0001: 使用 ADR 记录架构决策

- **状态 (Status):** accepted
- **日期 (Date):** 2026-06-28
- **决策者 (Deciders):** msuad + Claude

## 上下文 (Context)

本项目由 AI 与人类协作开发，多端（web / server / desktop）演进，技术栈含若干未定项。
我们需要一种机制，让"为什么选 X 而不选 Y"的推理**不随时间丢失**——否则：
- 新协作者（人或 AI）会反复重提已讨论过的决策。
- 违背原决策的改动会悄然发生而无人察觉。
- 决策的约束条件被遗忘，导致后续设计偏航。

## 决策 (Decision)

采用 **ADR（Architecture Decision Record）** 记录所有有意义的架构与技术栈决策。

- 每条 ADR 是 `docs/decisions/NNNN-short-title.md`，编号递增。
- 使用 [`0000-template.md`](0000-template.md) 的结构：上下文 → 决策 → 备选 → 后果。
- ADR 一经接受（accepted）**只追加不修改**；若推翻，新建一条并以 `superseded by` 链接。
- 未敲定的重大问题先记入 [`open-questions.md`](../open-questions.md)，敲定后产出对应 ADR。

## 备选方案 (Alternatives Considered)

- **只在代码注释里写** — 否决：注释随代码漂移、难索引、看不到历史决策。
- **写在单一 wiki 页** — 否决：长文档易腐、缺乏结构、决策间难交叉引用。
- **不记录** — 否决：这正是我们要避免的。

## 后果 (Consequences)

- **正面：** 决策可追溯；新协作者能快速理解"为什么"；避免重复讨论；约束条件显式化。
- **负面：** 每个重大决策需多写一份文档；需维护编号与状态流转的纪律。
- **后续需要做的：**
  - AI 协作规范要求：涉及架构/技术栈的改动须检查并更新 ADR（见 [`CLAUDE.md`](../../CLAUDE.md)）。
  - 推进 [`open-questions.md`](../open-questions.md) 中的开放项 → 转为 ADR。

## 关联 (Related)

- 模板：[`0000-template.md`](0000-template.md)
- 开放问题：[`open-questions.md`](../open-questions.md)
