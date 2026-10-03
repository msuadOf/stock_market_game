# 批次 175 独立复核

## 范围与来源完整性

本批按计划只审三份历史复核记录；逐份连续读取至 EOF，当前 SHA-256 和行数均与 `scan-plan.json` 一致。三份源文件均不在产品基线 `43b1aa5f25226c72976ca172d32f4a8eeb2272ad` 中，因此不能声称已从该提交恢复历史原始字节；完整性结论限于当前工作区字节与计划清单相符。

| 来源 | 行数 | SHA-256 | 读取状态 |
|---|---:|---|---|
| `agents/oop-refactor-audit/chinese-localization/before/exhaustive/reviews/engine-company-01-rereview.md` | 54 | `5e46f4f56a8574b93f21d18e79b637370144efc8304a55f02717f067a3cbdd6c` | EOF |
| `agents/oop-refactor-audit/chinese-localization/before/exhaustive/reviews/engine-company-01.md` | 32 | `64862be10c2f36704f52f480db2e296a51768092119984381f464d9863e312f7` | EOF |
| `agents/oop-refactor-audit/chinese-localization/before/exhaustive/reviews/engine-company-02-delta-first.md` | 35 | `2c2c67befc96c239377a0ebfd9aa839ac9febca82f463cb8f737441adb1611ee` | EOF |

产品对照使用计划基线 worktree `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`，与计划一致。已读 `AGENTS.md`、`docs/principles.md`、`docs/open-questions.md`、相关 `docs/decisions/`，并核对最新实现审计 `agents/implementation-audit/implementation-audit-2026-10-02.md` 的 G/Q 登记和下列现行实现入口。没有运行测试、构建或 Git 写操作。

## 三份记录结论

- `engine-company-01-rereview.md` 自身明确给出“未通过”，所列三类材料阻断具体且可证：遗漏 `bp_product`/基点常量符号、采购调用级数量溢出与子账直接失败混淆、调用者及测试定位空泛。其 D01 对负/零金额配对通过、D02 对 `receipt`/`write_off` 先改状态后 checked 累计的描述、D03 对 `close_year`/`correct` 多阶段提交次序，均与基线代码相符。该记录也明确将它们分类为会计行为风险而非 OOP 候选，没有据此声称交易制度变化。
- `engine-company-01.md` 标明为旧阻断的限定 delta 复核，并明说未重读 24 个源码、未运行测试；它将三个“审计材料阻断已闭合”与 D01–D03 行为缺口仍未修复分开，范围声明诚实。采购存档构造案例与 `IndustrialBooks::purchase` 的过账后收货次序相符。该结论只关闭文档记录精确度问题，不证明建议用例存在或运行通过。
- `engine-company-02-delta-first.md` 明确限于四个 delta 单元，没有冒充 25 个文件重审。它指出的模块总述“不可变投影”与公开可变 DTO / crate 内可写投影不相容；应视为审计材料表述冲突。源记录建议修正文案，没有把它提升成业务代码缺陷。

## 当前 owner、caller 与 consumer

- **合并抵销 D01：** `packages/engine/src/accounting/consolidation/eliminate.rs::precheck_balance` 检查成员、自指、科目和现金，`balance_entry` 检查镜像金额相等及资产/负债形状，但没有正金额检查；成对的零或负金额会进入工作底稿。`consolidation::consolidate` 是合并 owner，`accounting/reports/consolidated_window.rs::consolidated` 调用它，报告窗口及 `reports::generate_report_set` 消费结果。已有 G28 明确登记集团生产披露未接线和申报金额未校验账面上界；它与 D01 的非正金额校验不是同一断言，不能互相核销。当前 G/Q 主账没有明确登记这条非正金额候选，本批仅记录待由主审裁定，不直接更改主账。
- **工商采购/应收核销 D02：** `IndustrialBooks::purchase` 先 `post_with_commit`，随后 `inventory_mut().receipt`，再继续流量/应付子账；`CompanyOperations` 的工业日常路径是已见的生产 purchase caller。`write_off_receivable` 同样先过账再调用 `receivables_mut().write_off`，本基线生产源码搜索未见该公开入口的生产 caller。D02 讨论的是账套 API 的失败原子性，不是股票交易撮合或资金语义；现行 G70 是开局库存子账/总账对账问题，范围不同。历史建议用编辑存档构造极值，不可表述为常规旅程已触发。
- **结账 D03：** `ClosingEngine::close_year` 先执行并存储 12 月月报、关闭期间，再生成和存储年报；生产入口为 `GameSession` 日终封账路径（`session.rs` 调用 `close_year`）。`ClosingEngine::correct` 在 `Books::post_batch` 后记录 restatement、生成更正版；当前生产搜索未见 `correct` caller。现行 Q17 只登记 `correct` 在合法派生汇总溢出时可能部分提交，并明确未找到整 API 零改动保证/Session caller；它不能代表 `close_year` 分支已有确定可达故障或已完成验收。未发现本批来源提供可重复的故障注入/验证证据。

## 领域与 G/Q 门禁

本批涉及公司会计账簿及报表，不改变 A 股交易、结算、股份或资金单位语义。依据项目已有 `docs/company-accounting.md`，其中若干会计准则原文仍登记为取证受阻；本批没有新作 CAS 条款主张，也没有重新核验交易所或中国结算规则。适用架构方向与 ADR-0016 的公司经营/报告边界一致；ADR-0024 的投资者资金池决策与这些问题无直接关联。开放问题 Q17 仅与 `correct` 局部失败窗口相邻。

**结论：** 三份历史记录对自身复核范围、材料阻断状态及限定 delta 边界的表述总体可靠。确认当前仍有一个未在 G/Q 主账单独列出的 D01 非正往来申报候选；D02/D03 是已说明的公司账簿部分提交风险，但主账已有的 G70/Q17 各有更窄范围，不能据其编号暗示整项关闭或覆盖。此批不新增/核销 G/Q、不作产品改动；由主审决定 D01 是否达到正式登记门槛，并对 D03 的可达性另行取证。没有对沪深交易规则作新主张。

复核只覆盖三份分配记录和关键现行 caller/owner/consumer，不构成公司会计模块穷尽审查。未运行测试、构建或官方规则联网核查。
