# 会计账套契约复核

## 范围与方法

- 按要求全文阅读 `AGENTS.md`、`docs/principles.md`、`agents/implementation-audit/coverage/r03.md`、`docs/company-accounting.md`、`docs/company-actions-design.md`。
- 静态追踪 `Books`、`Journal`、`Ledger`、报表窗口/校验、行业账套写入及公开报告查询消费；未运行测试或完整回归。
- 审阅 `b89afb3..8cf34a1` 下 `packages/engine/src/accounting` 与 `packages/engine/src/company` 的完整 diff（51 个文件，含新增测试），未审阅无关领域 diff。该区间主要将会计/行业计算和校验收拢进 `RestatementRegister`、`ReportClassification`、`LoanPortfolio`、`ContractMeasurementState`、`BorrowingCostAccrualPlan` 等 owner；文档未改。

## 契约核对

- **输入与过账：** `JournalEntry::validate_invariants` 校验非空行、正金额、借贷双方及借贷相等；`Books::post_batch` 再校验批内/历史来源唯一、期间开放、科目存在、非现金分录不得触现金科目、金额运算及批末现金下限。先在克隆的 `Ledger` 上试算，全部成功后才替换余额并记录事实，失败以 `BatchAborted` 携带原因返回；空批是文档明确的 no-op。对应契约见 `packages/engine/src/accounting/mod.rs:84-124`。
- **账套事实与余额：** Journal 批次是持久事实；Ledger 是只读派生投影。`Books` 序列化仅保存科目表和 Journal，恢复逐批重放同一验证路径，再恢复封账状态；重复来源拒绝。行业写入包装器在 `post_batch` 成功后推进事件号；过账验证失败本身保持账套和事件号不变。过账后的子账应用另有失败面，见下方发现。
- **余额与报告：** 行业经营处理器通过 `Books::ledger()` 读取余额；报告生成从 Journal 分录纯函数构建有效期间余额/损益与实际期间现金流，再产出资产负债表、利润表、现金流量表、权益变动表和附注。`ReportSet::validate` 检查资产负债恒等式、权益/现金流勾稽、附注明细；无历史比较数据以 `Unavailable` 表示，不伪造零。与 `docs/company-accounting.md:135-154` 的 D1/D2/D7 简化说明相符。
- **领域边界：** 现有对外报告由 session 查询公开报告库，不直接将未披露账务投影暴露为玩家报告。公司行为文档 `docs/company-actions-design.md:39-65` 明确利润不自动分配、分配行恒零、总股本固定；生产报告权益项与此边界没有发现冲突。已登记的法源阻塞和版本化简化按文档保留，不视作实现缺口或法规不符。

## 发现

- **既有底层失败边界（非本次重构回归）：** 房地产借款计息仍先提交整批分录，再逐笔对贷款子账执行可能失败的 `apply_split`（`packages/engine/src/company/real_estate/borrowing_costs.rs:282-297`）。新增边界测试通过手工 serde 改写 `accrued_unpaid` 为 `AccountingAmount::MAX`，断言 apply 溢出时 Journal/事件号已变化而贷款与项目子账未变（`packages/engine/src/company/real_estate/ownership_tests.rs:424-455`）。逐项对照 `b89afb3` 的旧实现，顺序也是先 `post_with_commit`、再 `apply_split`、再聚合项目利息；本区间只是搬移/封装该流程。异常来自人工构造的极端 serde 子账值；本复核未在正常默认局或持久存档中复现，故不登记为本次新增缺口，也不把测试解释成产品承诺。

  `OperatingDayRun` 对阶段错误显式保留此前阶段已写入的结果，说明行业 operation 并非整体隐含事务；主 session 日终候选状态另有外层回滚保护，不能把上述直接行业 API 的局部失败外推到 session 日终。`Books::post_batch` 本身仍保证分录批次原子性。是否应把总账与行业子账更新升级为同一原子事务，属于需另证的现行契约问题；本轮证据不足以认定文档承诺必须提供该保证。

## 结论

静态追踪未发现本区间封装重构改变复式记账、来源唯一、现金流分类、期末余额、重述映射、比较项诚实性或报表呈列语义。记录的计息行为是旧实现已有的底层失败边界，而非本次重构新回归或已确认新增 G；`Books::post_batch` 的分录批次原子性不自动等于整个行业 operation 的账套与子账原子性。若要扩大原子性承诺，需另行确认正式契约并统一覆盖直接行业 API 与 session 日终边界。R03 已列的 G28/G35/G36 不在本次重复核定范围；本记录不代表完整实现验收或回归通过。
