# operations 独立复核记录

- 复核日期：2026-10-03。
- 基线：`b89afb3346743a4b4fccf26c9ac9ff108595f696`。
- 复核者 canonical 身份：`/root/implement_domain/review_operations`，未参与本批产品代码或保护测试实施。
- 权威需求：`agents/oop-refactor-audit/challenge-2026-10-03/action-index.md` 中 `domain-R2-N19`、`domain-R2-N20` 完整正文；实施交接见 `operations-result.md`。
- 完整产品 diff 范围：`packages/engine/src/company/operations/config.rs`、`core.rs`、`day.rs`；检查时无 operations 未跟踪源码。`dispatch.rs`、`injections.rs` 无基线差异，作为真实 caller/callee 接缝核对，未要求额外迁移。
- 已读 AGENTS、principles、architecture、testing、open-questions、ADR-0016/0023/0024、company-accounting 的依据与经营简化登记；检查完整三文件 diff、现行源码、旧日循环、到期派发/利息排队、相关工商流和已有四行业 determinism/失败测试。

## 最终版本绑定

2026-10-03 补证时重新读取当前 config/core 完整基线 diff 和 day 完整源码，核对与首次已审完整 diff 一致；三文件差量仍分别为 `50/0`、`8/17`、`584/104`（新增/删除行）。未发现后续内容变化，无新增发现；以下 SHA256 绑定本报告的最终静态审查结论。

| 文件 | SHA256 |
|---|---|
| `packages/engine/src/company/operations/config.rs` | `4165473d10b4e8c31b88894f9f6eab0dff7564636c43c3a8f8f22616c1fe8cc2` |
| `packages/engine/src/company/operations/core.rs` | `6ecd385334484d5de9dce920714db46f3ef2a73fcc2c36950bc3de982871c7e9` |
| `packages/engine/src/company/operations/day.rs` | `90da4e8b13b4151d0fb36267a0e188000ef8457a83628717d99b69052cfb5f9a` |

本次仅补复核文档与只读内容/哈希核对，未运行 Cargo 或测试；后续源码哈希变化将使本版本绑定失效。

## 结论

未发现本批需要修复的有效问题。独立静态复核完成；这不表示编译或测试已通过。按父任务约束未运行 Cargo、产品测试、全仓 formatter 或 Git 写命令；仅运行只读 `git diff --check`，无输出、退出码 0。

## 门禁 1：大 A 语义与依据

- `config.rs:97` 的四变体 `IndustryPairView` 只借用各自行业账套与参数，未混合收入/负债、会计账户或单位；`day.rs:249` 起的四行业委托参数逐项与基线一致。`AccountingAmount` 仍为分，经营商品 units 与证券 shares 未互换。
- `core.rs:190` 的顺序仍为 spec 校验、duration 校验、行业配对 guard；`config.rs:110` 只在 build 校验 spec.kind。`day.rs:308` 使用 `at_existing_guard`，仍只检查 books/params，保留既有公开 DTO/存档 Deserialize 接受集合、错误字段与错误时点，没有顺带修复可恢复的 spec.kind 错配。
- 推进仍使用逐自然日 `CivilDate`，不引入证券交易日跳日、T+1、板块申报差异。公司经营资金没有接入投资者账户，没有股东分红、融资、回购、清算分配或补钱行为。
- 本批没有新增交易制度或会计计算，因此无需新查官方规则即可判断重构没有改变制度。适用依据沿用 ADR-0016/0024 的公司与投资者资金隔离边界，以及 `docs/company-accounting.md` 的 2026-09-10 官方依据登记与 §2.6 `game-assumption-operations-pacing`；未独立联网重验官方原文，不将游戏经营节奏称为真实监管规则。

## 门禁 2：需求必要性与最小范围

- N19 的短期受检 pair 位于原 guard，随后消费强类型借用委托原行业 flow。长期 books/params 仍由 `OperatingCompany` 持有，未新增持久状态、公开 API、依赖或 trait。
- N20 的 `OperatingDayRun`（`day.rs:33`）拥有真实当日报告生命周期字段，借用原 `CompanyOperations`；`advance`（`day.rs:79`）明确组合阶段。没有复制 RNG、账簿、scheduler 或日期权威状态，符合可选动作限定的组合收益。
- `dispatch_due_actions`（`day.rs:150`）调用原 `CompanyOperations::dispatch_due_on`；合同 maturity 仍通过原 owner 的 companies 找到行业账套。`dispatch.rs` 中 AR/LN/DL 路由并非空壳或复制状态，不需要为完成 N20 再建 maturity owner。
- 改动局限于三个预期文件及七个短保护测试。头注改为实际顺序，消除旧注释与实现不一致，属于本次阶段提取必要说明。

## 门禁 3：边界、跨层与复杂度

- `day.rs:48` 保留 hash projection 先失效、日期守卫后拒绝；随后 journal entry 基数统计保持原时点。
- `day.rs:79` 保留 expire → 市场/行业/公司 sample 与 activate → dispatch_due → CompanyId 序 flow → date.next → 次日 interest → next_expected → 报告。行业采样仍先收集全部结果再激活，失败前的写入和 RNG 消耗时点保持。
- `day.rs:151` 仍执行原 scheduler pop/drain 和到期派发；dispatch 失败不回滚已 drain 队列/已处理账套。flow 失败不回滚先前 expiry、sampling、maturity 和其他公司 flow；不会提前排次日利息或推进日期。报告 entry 差额继续使用原 saturating_sub。
- 新七个测试分别锁定 duration 首错与 DTO 错配接受、恢复错配的先前阶段写入、day 无额外 spec.kind guard、记录/各路 RNG 顺序、错误日期的 cache 失效、当日 maturity 为后续费用供款/次日利息，以及 flow 错误后的既有部分写入。
- 四行业 delegate 逐项相同；已有四行业 determinism、shock/risk 金样、serde 恢复和 failures::flows 用例提供进一步回归入口。新增短测的经营 fixture 主要为 Industrial；Bank/Insurance/RealEstate 的动态行为需 root 执行已有集成测试确认。
- 日期上界、次日 scheduler submit 中途失败、dispatch 中途失败未新增专用测试；本批未改这些具体实现，静态检查未发现新的边界缺口，不能据此声称这些情景已运行验证。
- 无 UI/存档/API 形状变化，未发现跨层语义漂移或不必要的持久复杂度。`FlowDayContext` 只承接原调用参数和借用，未形成第二权威。

## 验证局限与交接

本记录仅核销本批独立静态审查门禁。root 仍须执行新 `company::operations::day::tests` 与相关 `company_operations` 回归，记录编译/执行结果；未执行结果不能登记为 passed。没有有效发现需要修复后复审；若上述三个文件后续变化，本结论需按新完整 diff 更新。
