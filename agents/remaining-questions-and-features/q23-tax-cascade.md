# Q23：历史更正与年度所得税级联

## 既有语义与风险

本批接续年度幂等处理器。用户要求更正结果以差额登记，失败允许重试，不创造退款现金。
`ClosingEngine` 将更正凭证实际过账到当前开放期间，同时把损益有效期间映射回目标历史
期间；税务重评必须读取同一有效期间，不能下一年再次按实际过账日期重复纳税。

只保存最新年度基准不足以重算已影响下一年亏损抵扣的更正。需要保存已评估各年度的
年度开始亏损池与累计结果；最早受影响年更新后依次重算后续年度，不直接修改期末池。
普通调用与更正调用共用同一年度税模型，继续显式 `IncomeTaxPolicy` 参数及已有 DTA
全额确认简化，不因工程级联扩成完整税法系统。

## 已实施的组合机制

1. `IncomeTaxPosition` 保存 `assessments`、最终 `loss_pool`、真实
   `initial_deferred_tax_asset` 和 `restatements`，不保留旧单年度格式。Web 当前结构
   同步严格键集、规范年度与来源键、金额字符串及有效期间；fixture 仍由 root 真实生成。
2. 普通计提更新请求年度并向后级联，从既有最早基准逐年生成新池、税额与 DTA；
   不重复消耗上一份本年期末池。较早但从未评估的年度没有可证明基准时明确报错。
3. 每个受影响年度的当期税差额与递延税运动差额过账到当前开放期间，同时记入对应
   历史年 12 月有效期间；这是既有年度年末计税的游戏口径，不伪称法定季度追补。
   实际现金期不变，已付款变化只形成待抵扣／应收事实，不发生退款现金。
4. `CompanyOperations::correct_industrial_report` 从真实 `CompanySpec` 解析公司与
   `Standalone` Scope，经 crate 内 `IndustrialBooks::correct_and_publish_with_tax`
   提供报表／税务更正组合入口：暂存行业账套、结账登记簿和
   公开库，预检经营调整、税重算、各有效期间、报表及公布；全部成功后一次替换。
   只有 `Books` 的通用更正 API 不知道行业税状态，工商税务更正须经经营 owner 入口。
   原始 Journal 调整不支持应收应付、存货、固定资产、贷款本金或税务 owner 科目
   的结构化子账变更；该类请求以 `StructuredCorrectionRequired` 原子拒绝，不能
   静默保留不完整子账。候选最后验证全部既有 Industry 恢复守卫再提交。
5. 保存恢复验证年度链连续的期初／期末池、累积税结果及重述来源，并将税务映射与
   同一个公司 Scope 的 `ClosingEngine` 映射交叉校验，避免两个消费者语义漂移。
   空评估链必须没有隐藏的 `TaxAccrual`，DTA 必须仍等于真实开局基准；不能通过
   清空年度链让已有税事实再次计提。低层登记映射的方法限 crate 内部，公共接口
   不暴露绕过结账登记簿的并行来源分配路径。

## 独立复核与后补边界

非作者完整 diff 复核发现三个 P1：空年度链隐藏既存税事实、未来凭证提前披露、
纯 Journal 子账更正导致成功后存档不能自恢复。三个问题均已修正并获增量静态 PASS；
新增日期／13 个受保护科目原子拒绝矩阵，恢复 case 拒绝清空基准但保留真实税凭证。
实际绿测仍由 root 统一编译后运行，不把静态 PASS 当验收。

已获 root 确认 CAS 18 第十条超缴应列资产。独立真实旧产物 driver
`q23-tax-asset-red.rs` 在现金不变检查通过后，因缺少 `CurrentTaxAssets` 行实际红
（exit 101）；随后改为 BS／notes 在各自窗口按 `222104` 真实借方重分类，工业投影
及 Web 结构同步。root build5 刷新后，对应 exact case 实际绿（0.01 秒）。
不以账务平衡代替正确列报，不自动现金退税；Consolidated 的成员不抵销仍待专项
红绿与独立复核。已公开的后续报告按真实新有效期间生成结果比较，变化才生成
`Correction` 并关联原公开 ID，原版保持不变。已公开且期末结束的 Quarter／HalfYear
快照允许专项重述，不为它补假封账；未公开、非法时点或错 Scope 不取得这个例外，
Monthly 仍保持真实封账守卫；已公开且期末结束的 Annual 前史快照与 Quarter／HalfYear
采用同一不可变公开版本验证，允许历史重述，不给原 Journal 补造封账状态。未公开
Annual 与普通低层更正仍需要真实封账。

已经定稿但尚未公开的原始报告也必须跟随重述：`ensure_original_registered` 比较
真实新窗口，变化时追加新的私有 `Original` 而不覆写旧版，返回实际 `sequence`，
前史／日终发布不再写死 1。首次公开 `sequence > 1` 仍是首次原始报告，
不伪装为已公开更正、不把旧私有版泄漏进公开库，普通信息／更正获知以公开 origin 区分。
这些后补链仍待实际短测及完整复核，不宣称完整 Q23 完成。

公开 `CompanyOperations::correct_industrial_report` 仍拒绝缺少集团外层的 root／child
独立调用，避免单体成功而留下旧合并结果；`GameSession` 受控日终候选入口联动所有
单体／集团版本后一次安装，相关真实短测正在验证，见
[集团更正](q23-group-tax-correction.md)。这不是“通过拒绝实现集团更正”。月报排期
必须使用用户确认后的明确方案，不由税务重算擅自选择信息日程。

首批只有 Industrial 税 Owner，后续已将年度 `IncomeTaxPosition`、显式政策、计提、
真实现金付款及严格恢复抽成四行业共用机制，Bank、Insurance、RealEstate 不再用
三套复制实现；各自 Owner 的真实短测已通过。`IndustryBooks` 统一分派，日终应用
更正使用共享纯候选并由实际 Owner 完整验证后一次安装；混合集团不共享成员亏损池
或税资产。四种列报纯候选的成功不能代替真实专属业务更正证据：保险的三类损益
涉及合同组，地产涉及项目／预售／贷款，原始 Journal 对相关科目仍明确业务拒绝。
这些结构化业务边界与地产完整子账守卫单独验收，不作为全部 Q23 已完备的核销理由。

## 官方依据与经济边界

官方补证由 `q23-tax-official-research.md` 登记。已取得税务总局现行企业所得税法全文：
第五十三条公历年度，第五十四条分月／季预缴与年度汇算，第十八条普通亏损结转，
第二十一条税会差异按税法。本游戏已有 `TaxPolicy` 和简化税会一致税基不因此自动成为
法定完整处理，不能把报表月／季频率等同预缴频率。法定优惠、滞纳金、征管程序、税会
差异及税款实际退款仍没有由本批工程调整自动得到实现授权或公式。

## 红测与当前验证入口

先把 `tax_state_restore_requires_annual_baseline_and_rejects_inconsistent_pool` 的末段
改为已评估 2031 后，重复无变化的 2030 评估必须不新增事件且不改变任何状态；现有
旧 `HistoricalTaxReassessmentUnsupported` 会失败。root 统一编译 exact case 尚未
闭环前，作者用 root 已有 `libengine-5522527e6b85936a.rlib` 链接
`q23-tax-cascade-red.rs` 独立 driver，工业税生产 `expenses.rs`／`mod.rs` 当时与
HEAD 无 diff；`rustc` 编译和运行各用外部 10000ms deadline，实际因该旧年度拒绝
返回 exit 101。该结果是既有真实产物行为证据，不冒充当前新源码 exact 绿测。

当前新增 Engine 确切短测包括三年级联／实缴不退款／继续新年度不重复计入历史更正，
真实 Books／Closing／PublicLibrary 成功更正与错相位整批回滚后同 source 重试，
未来过账与未支持结构化子账变更原子拒绝。原年度幂等金样仍须保持通过。
