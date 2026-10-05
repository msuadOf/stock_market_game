# Q23：中期增长材料的同窗口约束

报告种类排序和年度基准刷新后继续检查 `extract_interim_growth`，发现原读取组合不一致：`income.cumulative` 是当年1月至报告月累计，而 `income.prior_year` 来自 `ReportKind::resolve` 的上年同报告窗口。月报2月会拿本年1–2月对上年2月；三季报会拿本年1–9月对上年7–9月。1月及一季度的窗口重合，不能用它们证明口径一致。

用户已要求月报可配置、信息由本人实际获得，并按真实公司业绩解释；本修补只保持既有 `OperatingRevenue` 增长度量的同窗语义，不改银行／保险收入分类、个人增长学习参数或估值方法。root 已批准复用公开 `Notes.items` 的 `Income(OperatingRevenue)` 行 `movement`：该字段来自真正报告窗口，借方为正／贷方为负，取收入时须 checked 反号并合计；上年列则已经是同报告窗口。不得用 `ytd_movement`，也不把 `consolidation_split_items` 再合计。没有新增存档字段或兼容格式。

已先新增 `interim_window_tests` 的4个真实财务短夹具：月报2月、三季报，分别覆盖单体与真实固定80%持股的合并范围。往年1月、2月、5月和9月均有10元收入，本年相应月份为11元；真实 `Journal`、`ReportSource`、报表生成及 `ReportSet::validate` 均参与。正确同窗同比是1000bp，原错误累计比较会达到既有2000bp上限；这样测试能区分真错误，不能靠 clamp 假绿。

最初拟用20%同比，但非作者复核发现它恰处于2000bp上限，正确结果与错误累计结果都会被钳为同值；已在生产修补前改为10%，没有放宽或改变参数。root31 没有编入这些新 case，因此不能把其他月报／年度基准短测的通过借作本项通过。root32 实际运行四项确切 case，均得到旧算法 `TwoYear(2000)` 对正确 `TwoYear(1000)` 的业务红；随后 `facts.rs` 改为主附注窗口运动的 checked 汇总，溢出显式返回既有错误，不改上年列、分类或增长学习参数。有效零行省略依赖现有 `ReportSet::validate` 勾稽，不将缺历史补为零。

root33 重编后四项确切 case 均实际执行并通过，与32四红一一对应；同批增强年度基准 case 也再次通过。五个短测进程并行、每进程 Rayon 8线程、外部10秒 deadline，整批 wall 0.105秒。非作者亲读四红四绿和完整限定源码后，确认同窗修改 Gate PASS，详见 `q23-interim-window-review.md`。此结论仅覆盖同窗增量，不代表整个 Q23、经营驱动或完整回归完成；实际 WASM、发布 fixture 和最终存档接线仍由 root 统一收口。
