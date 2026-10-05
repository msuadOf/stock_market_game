# Q23 当期所得税超缴资产列报

## 依据与范围

2026-10-05 已取得的财政部 CAS 18 页面第十条要求将当期及以前期间应交未交所得税
确认为负债，将已支付超过应支付部分确认为资产。原文、来源及不完整正文边界见
[所得税官方研究](q23-tax-official-research.md)。本轮不把部分原文取得冒称准则全文核验，
也不增加现金退款、税率、法定预缴日程或税收优惠。

`222104` 的真实净借方代表超缴的当期所得税资产；净贷方代表应交所得税负债。
不同成员是不同纳税主体，合并报表不能先将其相反余额互抵，再按集团净额择一列报。
本轮只修读投影，不造重分类 Journal，不改变原账务或 CashFlow。

## 合并报表方案与短测试

报表窗口为每个成员的 `222104` 保留独立可解释键。窗口与列报分类共用同一键映射；
资产负债表和附注继续读取同一窗口余额，避免建立重复的 gross 余额权威。
成员标识采用长度编码，保留原始科目身份，其他科目的既有汇总规则不变。
期末与上年年末分别按各自真实余额分类，不用期末分类倒灌比较期间。

代表性短测试为 `consolidated_gold::consolidated_current_tax_balances_preserve_separate_taxpayers_and_prior_year`。
覆盖两个纳税主体相反余额等额、超缴较大、应交较大三种情况，核对当前主表、
上年比较项、附注明细、资产／负债总额及现金余额。
行为红已使用 root 统一构建的真实 `industry_reports-f74043c053d1770f` 执行：
先核对 `--list` 存在对应 case，再以外部十秒 deadline、`RAYON_NUM_THREADS=16` 运行
exact case。实际执行一项并失败，原因是等额超缴／应交被先互抵，缺少 `CurrentTaxAssets`
行，耗时 0.00 秒；日志为 `.tmp/checklist-wave4/consolidated-tax-assets-red.log`。

红测后已实现 `report_account_keys` 为每个成员的税科目保留独立键，原科目表和日记账
均不改变；附注科目名称包含成员身份。`ReportClassification` 使用同一映射，
`effective_target` 按各窗口的成员税余额决定资产／负债归属。另增
`tax_member_keys_preserve_unicode_and_delimiter_identity` 验证成员包含中文、冒号及
科目样式后缀时键仍不混淆，错误长度编码不被识别为税键。
root 统一编译成功，耗时 71 秒；产物登记为
`.tmp/checklist-wave4/tax-assets-current-binaries.json`。两个 exact case 各先核对真实
binary 的 `--list`，随后两个进程并行、每进程 `RAYON_NUM_THREADS=16` 并使用外部十秒
deadline 执行。合并税余额 case 实际 1/1 通过、0.01 秒，成员键 case 实际 1/1
通过、0.00 秒；已亲读对应日志：
`.tmp/checklist-wave4/consolidated-tax-assets-current-green.log` 与
`.tmp/checklist-wave4/tax-member-key-current-green.log`。
相邻 `consolidated_gold::` 七项短测（含本项）全部通过，耗时 0.01 秒；使用外部十秒
deadline、`RAYON_NUM_THREADS=16`、`--test-threads=8`，与另一独立 Engine 测试进程
并行执行。已亲读 `.tmp/checklist-wave4/consolidated-tax-assets-current-adjacent.log`。
非作者已独立审读上述完整税资产 diff 和三份绿色日志，限定静态复核通过；同时指出
“比较期间与本期税余额方向反转”未被原测试覆盖。已补
`consolidated_tax_assets_classify_prior_and_current_balances_independently`：母公司从上年
超缴 30 元变为本期应交 20 元，子公司从上年应交 40 元变为本期超缴 10 元，分别
核对两期真实 gross、附注开期／运动／闭期及现金不变。root 增量统一编译耗时
4.14 秒，确认 fresh binary 的 `--list` 有该 case 后，以外部十秒 deadline 实际执行
一项并通过，耗时 0.00 秒；已亲读 `.tmp/checklist-wave4/tax-assets-direction-green.log`，
编译记录为 `tax-assets-direction-build.jsonl`／`.stderr`。八项不同 integration case
分两次短命令实际通过，加上成员键一项；不是一次全仓或全套回归。
非作者已复核新测试源码、实际一项绿色日志及隔离暂存 patch 全文，最终限定签核
通过，前述测试遗漏已闭环。隔离 patch 只包含本项税键、列报、附注、两项集团
金样和 Web 的 `CurrentTaxAssets` 字面量，不包含并行集团重述、财报频率或交易日历
改动。上述结果不是完整回归或完整税法合规声明。

最终提交补入独立单体列报 case
`standalone_current_tax_assets_keep_deferred_assets_and_each_comparison_direction_separate`，
仅使用既有 `Books`／`ReportRequest`／工业列报投影 API 与空重述映射，不依赖尚未
提交的年度税 Owner 或级联接口。两个方向分别验证当期／比较期所得税资产与负债、
`DeferredTaxAssets` 50 分不混同、增值税余额不与所得税超缴互抵、原现金不变及报表
勾稽。该纯列报夹具不是行业税务处理器的合法输入承诺；测试是在生产实现已有后补入，
不冒称此新增 case 自身先取得业务红。host43 的金额 `.abs()` 类型推断编译失败仅是
夹具编译问题，显式 `i128` 修复不改变金额或断言，也不计为业务红。

root 的 host44 fresh 五 crate `no-run` 编译通过后，实际定向运行单体 1/1、集团
4/4 和成员键 1/1，分别耗时 0.00、0.01、0.00 秒；非作者亲读
`tax-assets-standalone-host44-green.log`、`tax-assets-consolidated-host44-green.log` 与
`tax-assets-key-host44-green.log`，均位于 `.tmp/checklist-wave4/`。这些是短测，不是回归。
非作者最后完整审读真实 `git diff --cached` 的七个源码／测试文件：仅税资产 enum、
列报／附注、成员税 gross 独立键、两个集团金样及单体夹具，无并行级联、月报或 Q13
改动，函数与测试依赖已存在的 HEAD API。两份隔离 patch 的零上下文 hunk 必须使用
`--unidiff-zero` 校验，该要求不表示 HEAD 漂移。最终限定提交门禁 **PASS**。

本项只负责税资产列报，不借此修改或认证集团历史更正能力；税务级联属于独立任务，
其实现及验证不得据本项签核核销。
