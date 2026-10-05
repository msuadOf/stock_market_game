# Q23：历史报表覆盖事实校验

## 问题与修复

`prior_year_facts(books, period)` 只判断实际账套是否覆盖上年流量及上年末余额，不读取有效期间重述映射。
上年末余额不要求十二月恰好发生凭证；截止日前已有实际账套事实即可证明余额存在。
更早的真实 `OpeningBalance` 证明连续账套，包括全年没有业务的已知零流量。
当期才开账或晚过账的凭证，即使重述到过去，也不能制造开账前的历史覆盖。

报表金额仍由原有有效期间映射计算；本批不改变 Journal、现金、所得税率、税额或交易制度，
不引入版本兼容、迁移或默认补字段。函数保持原来的双参数接口，避免把金额重述与覆盖事实混为一谈。

## 定向证据

- 首轮新增测试在 `host17` 编译时暴露 `close_year` 返回两个 `ReportHandle` 的测试解构错误；该编译失败不算业务红测。
- `host18` 实际短测取得两个业务红：上年一月开账却没有十二月凭证被判为缺失；仅当期实际账套被有效期间映射误判为存在过去历史。更早开账后追加重述的正控制通过。
- 根修后 `host19` 三项通过。移除无用映射参数后，`host24` 再次实际执行三个 exact case，全部通过：一月开账年末余额、连续空年度与晚记费用、禁止制造开账前覆盖。
- 最新日志为 `.tmp/checklist-wave4/host24-january_opening_without_december_entries_still_has_real_prior_year_end_balance.log`、`.tmp/checklist-wave4/host24-earlier_real_opening_keeps_empty_year_coverage_when_late_expense_restates_amounts.log`、`.tmp/checklist-wave4/host24-mapping_current_year_entries_to_prior_year_cannot_manufacture_history_coverage.log`。
- 批次使用独立进程并行、每进程 `RAYON_NUM_THREADS=8`，case 与整命令均受进程外 `10000ms` deadline；最新批次还包含集团正例和另一个月报待修反例，不把批次总体失败冒充全部通过。

非作者 `review_q23_income_tax` 最终完整复核本批 `validate.rs` diff、本文及上述三项
`host24` 实际日志：每项均真实运行一个 case 并通过，分别耗时 0.01、0.00、0.00 秒。
函数保持旧 HEAD 双参数接口，测试只消费旧 HEAD 已有 `ReportRequest`／`Standalone`／
`adjustments` 类型，不依赖新月报或集团成员接口；修复必要且范围最小，未削弱断言。
本批最终独立复核 **PASS**，不将同批其他失败或此前编译错误包装为业务红／绿。
本批只证明历史覆盖守卫，不核销整个 Q23，不代替四行业税务、月报或完整回归验收。
