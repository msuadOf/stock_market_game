# 普通税日实际来源薄接缝

日期：2026-10-27。待独立设计确认；不是 Bonus 日简化。
唯一入口消费真实 `ShareRegistry before`、`ShareSettlementDay` 与 `ShareMarketSourceContext`，内部调用既有 `prepare_settlement_day` 核 calendar、完成水位、最终确认全集、前驱及物理净额，不另起来源 authority。
完整历史也不得含任何公司/单位 movement（包括旧 scope 与 typed binding opening/post movement），不能从本日无 movement 冒历史税日/单位已证。
支持仅 ordinary market 与同日 CaptureRegistration（不改变数量）的已证范围。存在 opening/unit/post movement 时，返回带 event/day 的明确来源缺口，保持原生产 Bonus 门，不能按 R+1 日期默认转换。
借 `CashDividendTaxBook` validate、检查账户/证券/期初日与完整数量；期初税日严格等于REG前驱日。税lots必须与受控装配器从完整registry lots逐项投影得到的id/qty/date/source/class完全一致，不仅同总数/同日期，也不靠id前缀冒同源。仅Unrestricted与PublicMarket，不猜restricted资格。所有新增 lot 仅从受核 PublicMarket 实际正净额对应 SecondaryMarket 来源取得，零/负净额不制造 acquisition。账户未在changes中出现或CalendarClosed时也逐日record_net_day(...,0,None)。用 `record_net_day` 生成候选，期末税数量须与受核普通 registry 一致。
输出只读 `TaxDayBoundaryFacts` 含受核 registry candidate 与 tax book candidate，源 event/day/receipt frontier；不持久化第二份权威。
Capture必须为本请求post中声明的实际plan_id，严格同日且settled_receipts等于candidate末端、Initial unit；不再另造R id或二次register绕过typed capture声明。
R Capture 通过完整税日 frontier 与当时实体账户持有总量一一相同来建立 `TaxRegistrationAttribution`（完整税lots），不以字符串lot前缀或比例猜映射。仅前述不含任何公司movement的普通同日路径可确认这一全集关系。physical90/tax100在入口期初或capture精确拒绝，不把tail100全部视作physical90原权益。
如果90/100来自真正中间capture或单位路径，当前无足够来源事实完成此attribution，需要真实完整税务日/中间归属；本首片不声称覆盖。
真实World fixture拟：真实成交→实际日志归一确认→受核REG request→税book日净额+R；源日/水位缺失、确认遗漏、postmovement、90/100均拒绝且输入不变。禁止伪造AdminCandidate；这条来源薄接缝不应用Admin操作。
此入口由R4唯一生产组装器后续消费，生产caller尚无；不能当作长期叶终点。
依据：财税2012〔85〕第三条及中登2012-11-30通知第五条的每日日终记录净增减；101号更新税率/暂不扣缴，本片不更改税率或收缴。


2026-10-28独审修订：唯一入口改为 `prepare(registry, books:&[CashDividendTaxBook], day, sources)`，每证券只准备一份REG，输出`books()`按book.account排序，重复账户/证券错配明确拒。不保留平行单book入口。实际批索引仅借当前来源，不成为持久authority。
每book从自己的opened_on受核完成日anchor开始，将后续每日日终净额/acquisition/dispositions与REG逐项绑定，逆推opening完整lot；不能仅靠tail相同推历史。保留首次入册账户前一天空book的既有维护钩子，不在此放宽原生产configure禁历史补账。
归属字段private且只读getters，不允许其他模块手造TaxRegistrationAttribution。R4唯一owner已接受此批签名；其真实E2 FullInstalled→确认/REG组装→本入口→同candidate安装仍未实现。
