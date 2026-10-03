# 批次 182：engine-foundation 历史复核记录

## 范围与方法

本批按唯一扫描计划审读三份历史审查材料，均逐篇读至 EOF，并核对行数与 SHA-256。对照了 HEAD `43b1aa5` 下的批次 181 复核、当前 `implementation-audit-2026-10-02.md` 总账、`reaudit-engine.md` 与 ADR-0023 至 ADR-0028 的索引/决定主题，并阅读目标 worktree 的 `AGENTS.md`、`docs/principles.md`。本批只复核历史材料的现行适用性，不重做其中记录的源码全文审查或测试，也不运行测试、构建或 Git 命令。

## 材料核对

- `engine-foundation-03-recheck3.md` 报告 engine-foundation-03 的 inventory、撮合失败边界和 JSON/Markdown 一致性已通过复核。它本身依赖既有 items/modules 与 delta evidence，不构成独立的当前生产行为承诺；其“无需阻止本批”的结论只适用于该 OOP 归档复核范围。
- `engine-foundation-03.md` 汇总了旧审查与 `unit-002` / `unit-044` 的增量结论，明确限定普通撮合和若干恢复/投影路径的失败提交边界，并说明没有重新读源码。该限制及“未运行测试”均有明确披露。文中把 `Market::set_last_price`、`Market::set_last_close` 和 `record_filled_order` 称作公开写口的描述，不能套用到本次基线：批次 181 对 HEAD `43b1aa5` 的核对确认它们在 `market.rs` 中为 crate-private。此处属于历史材料的基线漂移，引用时须保留其历史范围，不得作为当前 public API 或跨层契约证据。
- `engine-foundation-04-recheck.md` 报告对计时模块和支撑测试文件的归档覆盖通过，明确测试 fixture 不扩写交易规则，且没有运行测试。此结论确认的是旧 inventory / module 文档与当时源码相符，不证明当前行为、完整调用链或测试现状。

## Owner、调用与契约边界

批次 181 对相关 owner、caller 和 consumer 的基线核对表明：`OrderBook::place` 的错误可能保留先前簿内变更；`Market::place_inner` 只有在簿操作成功后更新末笔成交价；candidate restore 的局部提交保证不能推广到 `apply_changes` 或 filled restore。其调用证据包括连续撮合及 session/pipeline 路径，但不声称穷尽所有 callers。`verification_evidence` 的 owner / re-export / 使用边界也没有支持提取新对象的已接受承诺。以上是对前序复核的引用，不是本批重新检查这些源码得出的新结论。

## 总账、ADR 与裁定

总账记录 G01–G68 等当前实现缺口，并明确区分缺口、未决契约和错误原子性观察；本批三篇历史 OOP 审查没有映射出新的已接受实现承诺，也没有理由把部分失败状态变化升级为全操作事务缺口。相关 `Q11`、`Q12` 已分别由既有决定处理。ADR-0023 至 ADR-0028 讨论虚拟历史/撮合范围、资金池、日终持久化、机构行为及部署发布，没有改变本批涉及的 `OrderBook` / `Market` 失败语义或 `verification_evidence` owner 边界。没有发现与 A 股撮合语义冲突的新主张；本批不重新裁定或扩展交易规则。

## 结论

未发现基于这三篇材料可新增的 G/Q 或 OOP 提取候选。保留历史材料对撮合部分变更及提交边界的限定；引用 Market setter / `record_filled_order` 可见性时须采用 HEAD `43b1aa5` 的 crate-private 现状，不能照抄旧“public”描述。没有发现遗漏的已接受承诺、历史错误核销或新的跨层语义漂移。
