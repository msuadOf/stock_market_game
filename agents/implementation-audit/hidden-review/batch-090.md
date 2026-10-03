# 批次 090 隐藏复核

## 范围与方法

- Batch：90，owner：5；基线：`43b1aa5f25226c72976ca172d32f4a8eeb2272ad`；源根：`/data1/baiyifan/workplace/stock_market_game`。
- 当前调用方规范：`.worktree/implementation-reaudit/AGENTS.md` 与 `.worktree/implementation-reaudit/docs/principles.md` 已读取。开放问题与决策核对 `docs/open-questions.md`、最新 ADR-0028，以及相关 ADR-0016、ADR-0024、ADR-0025、ADR-0026。
- 三份来源均为中文化审查记录；它们把结论限定为语言变化保持性，不声称重新审计实现或通过产品测试。按该范围核对其声明及当前基线的选定调用/所有权事实；不把旧审计中的候选建议升级为本轮发现。

## 源材料 EOF / 指纹

| 来源 | 行数 | SHA-256 | aliases | 读取状态 |
|---|---:|---|---:|---|
| `agents/oop-refactor-audit/chinese-localization/batch-reviews/engine-company-05.md` | 22 | `86f350109c512f707b5678887809b1a0b7b98b84836a43bc473c7048b8dc373d` | 1 | 连续全文读取至 EOF；清单行数、摘要匹配 |
| `agents/oop-refactor-audit/chinese-localization/batch-reviews/engine-foundation-01.md` | 22 | `760b3f90531dd548c2016f71f68249b79991016f63abb6fe871d0d15c6be2470` | 1 | 连续全文读取至 EOF；清单行数、摘要匹配 |
| `agents/oop-refactor-audit/chinese-localization/batch-reviews/engine-foundation-02.md` | 22 | `d8a0d9313b53e802d7f904434df354eb24b4b076ec538b72558caa15167f1ecd` | 1 | 连续全文读取至 EOF；清单行数、摘要匹配 |

逐份完整核对，未发现清单外尾部或别名歧义。材料仅继承其引用原审查的结论，并明确原审查身份与范围不因翻译复核扩大。

## 核查结论

**没有确认的遗漏或错误核销。** 三份记录均准确区分中文化差异审查、源码风险记录和产品验收；明确未改源码、未运行测试/构建、未重新核验官方规则。公司与基础域的原始审计中存在的未实施候选和显式风险仍被声明保留，没有被改写为已修复或已验收。

- 当前调用/所有权核对：公司 `OperatingScheduler` 持有 `next_seq`、`settled_through` 与 `pending`；公司动作通过 `CompanyOperations::dispatch_due_on` 调用 `pop_due_on`，会话时钟接线使用 `due.id.value()`。基线 `scheduler.rs` 的 `submit` 以 `next_seq += 1` 分配 ID，`from_parts` 验证排序、key 唯一和序号范围；所述 duplicate `ScheduledDueId` 与溢出风险仍是独立源码风险，不与对象归属混为一谈。
- 公司经营边界核对：工业库存由 `IndustrialReconciliation::seed_inventory_and_assets` 建立；`available_credit` 是 `IndustrialBooks` 的查询入口。材料保留 `seed_inventory`、显式错误和保险局部失败等既有风险边界，没有承诺本次修复它们。公司排期不支持股东分配，不能据公司现金流暗示投资者注资或资金回流。
- 基础域核对：日历 `CalendarPolicy` 以 `compute_content_digest` 校验内容，覆盖通知文本与来源 digest 的边界仍列作源码风险；账户买卖结算确实在佣金、印花税之外处理 `transfer_fee`。GPU `decide_all` 当前委托 `CpuBackend`，工厂 `try_create_gpu` 可选创建，报告明确 GPU 非权威计算且未证明 parity。上述风险不能由中文复核“通过”结论消除。
- 决策/大 A 语义：Q1/Q2/Q5/Q6/Q8/Q9/Q11/Q12 在 `open-questions.md` 均有已决边界；相关公司信息方向见 ADR-0016，日终存档语义见 ADR-0025，投资者资金池可减少见 ADR-0024。最新 ADR-0028 是发布与静态 Pages 决策，与这三份材料的领域结论无冲突。材料不引入交易制度变更；没有新增官方规则取证需求，不能将本次结果当作规则正确性认证。

没有把代码中模块存在等同于行为已正确，也没有把历史通过状态扩大到当前实现完整验证。未运行源码测试、构建或回归；本次仅核对材料、基线调用/所有权及决策语义。
