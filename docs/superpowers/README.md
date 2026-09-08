# 历史设计记录

本目录中的 `specs/` 与 `plans/` 保存实现过程中的历史方案和任务分解，用于追溯，**不是当前交易规则或架构的权威来源**。其中可能保留已经被后续 ADR 和实现取代的 T+0、可配置涨跌幅、银行家舍入等旧假设。

当前规则以 [`../trading-rules.md`](../trading-rules.md)、[`../architecture.md`](../architecture.md)、[`../decisions/`](../decisions/) 和通过测试的 engine 实现为准。修改历史文件不应被用来改变现行规则；规则变更必须同时更新权威文档、ADR、实现与测试。
