# 批次 177 独立复核

## 范围与来源核验

按唯一计划读取三篇材料全文至 EOF。批次计划、SHA-256 与行数核验一致；基线 HEAD 为 `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`。三篇均为 `chinese-localization/before/exhaustive/reviews/` 下的历史复核记录，并非产品代码或直接实现证据。

- `engine-company-04.md`（23 行，SHA-256 `641155d6ee5c8de68206999ef479f857afbcedaa1919eeec531fe118602319c8`）：复核 company query、核心状态 owner 与地产模块三条审计材料 delta，强调局部调用/提交边界，未将候选当成源码修复。
- `engine-company-05.md`（31 行，SHA-256 `6e7ee0a53e25b7b3be55b54bdd84ac6042369a7f1b9755f81fc04b73bb8cbe56`）：复核 scheduler 与 spec 清单，保留恢复时 `ScheduledDueId` 重复和 `next_seq` 耗尽两项风险，并明确未修复、未运行测试。
- `engine-foundation-01-delta-a.md`（38 行，SHA-256 `97434123556e0c34d6167db264231ba9b74cf924bcc269239c2eba6e2b620443`）：复核 Account、NPC decision、GameConfig 三条清单 delta；提出当前 item 仍称 RNG 驱动 decision 为 “pure decision function” 的措辞不一致，并指出公开/serde 恢复边界。

## 对照与候选

对照当前实现审计总账、engine 复核和 ADR-0006、ADR-0016。总账已把公司经营缺口限定在真实生产闭环（如 G35/G36），NPC 意图、个人信息及撮合边界按 ADR-0016 处理；ADR-0006 规定 NPC 策略与交易账户解耦。以上原则支持区分经营/决策模块与真实交易语义，不能由历史 OOP 复核结论推导沪深制度变化。

- **engine-company-04：** 三处 delta 是对清单描述范围的收窄；报告分别强调 From 投影/checked 聚合、缓存失效写口、地产 `post_with_commit` 的局部提交语义。现行总账未把这三项作为新增已批准承诺或修复完成证明。其材料提及的边界值得作为后续源码核对候选；本文没有重新验证相关生产 caller 与全部写入 consumer，故不升格缺口。
- **engine-company-05：** 重复待办 ID 和序号耗尽是记录明确列出的未修源码风险，值得在现行实现核对时关注。当前总账 G35/G36 等公司经营条目不覆盖 scheduler ID 唯一性或序号耗尽；但这三篇历史复核没有证明其生产可达性、caller 与持久化恢复 consumer，本批也未读取源码，故只记录为候选，不新增或升级 G/Q。
- **engine-foundation-01 delta-a：** “pure decision function”与显式 `&mut dyn Rng` 的消耗描述存在术语张力；作为审计清单用语问题可保留为候选。Account 的 serde/公开可变边界也需按实际 restore caller 与状态 consumer 判断。该材料明确未复核所有恢复路径，故不能据此认定存档已有非法状态入口，也不改变总账状态。

## 结论

未发现可由这三篇历史复核记录单独确认的已批准承诺遗漏或错误历史核销。记录中的实现风险和措辞问题均保留为候选；不自行升级 G/Q，不宣称风险已修复或测试已通过。本批未重审生产实现、调用链或存档 consumer，未查询官方交易规则。材料涉及经营、账户封装与 NPC 决策审计，不提出新的 A 股交易制度结论。
