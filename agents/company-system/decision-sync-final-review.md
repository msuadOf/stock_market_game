# CompanySystem 决策同步独立复核

日期：2026-10-06

## 范围与结论

已全文阅读以下文档及其 `HEAD` 到工作区的完整差异：

- `docs/decisions/0035-company-system-simple-fundamentals.md`
- `docs/open-questions.md`
- `docs/company-accounting.md`
- `docs/work-status.md`
- `agents/company-system/implementation-checklist.md`
- `agents/remaining-questions-and-features/q14-financial-model-design.md`
- `agents/remaining-questions-and-features/q14-economic-parameters.md`
- `agents/remaining-questions-and-features/monthly-company-drivers.md`

大部分同步符合最终许可：simple 初值可编辑、可按 seed 复现的虚拟参数；可用股价、股份数量及显式虚拟估值倍率作一次性初始化反推；周期可选月／季度／半年／年，扰动按各周期配置；不强制月度内部生成；公司资金仅是展示账面值，无 `SyntheticFunding` 或真实公司资金流；Simulation 经营与客户资金 dispatch 留给后续分支。较长结算周期缺少细期间事实时不得均分、伪造资料或静默跳过披露。

## 发现

1. **初始化校准尚未成为可执行验收要求。** ADR-0035 §决定及 Q14 蓝图的讨论沿革提到可由股价、股份数量和显式虚拟估值倍率反推初值，且限定为初始化、不在后续重锚；但 Q14 正文“初始化和一轮完整例子”步骤仍是选择业务、指定现金/设备/库存/债务等，未纳入这一 simple 初始化分支。`implementation-checklist.md` 的实现事项、TDD 验收也没有覆盖此路径。因而核心许可虽在决策文字中出现，工程实施仍可能遗漏，亦未明确多个反推指标如何与收入、利润及权益状态一致。建议把“初始化一次性校准”及其边界和测试加入正式实施清单/蓝图；如属实现在做决定，同步定义估值倍率输入和反推字段，不能默认真实 PE/PB 或真实统计。

## 语义与范围复核

- A 股方面，本批文档不更改交易制度、撮合或公司行为规则；清单仍要求实施阶段按适用官方规则核验，公司类别与证券板块保持区分。未发现新增规则依据或事实性合规声明。
- 分红明确以可分配利润/方案等条件决定，不把账面展示现金当拒绝条件；实际持有人到账与 simple 公司资金来源链分开。这符合本轮用户许可，但必须维持“简化账面模型”边界，不把展示现金描述成实际付款来源。
- 周期与披露排期分开；较短报告期间不存在已结算事实时标为不可用并说明原因，避免年度流量伪装成季度事实。符合许可。
- `q14-economic-parameters.md` 将旧月度示例保留为历史提案，并明确不构成 simple 固定周期；`monthly-company-drivers.md` 将月度驱动限定为未来 CompanySimulation 经营方案。二者与四种 simple 周期没有实质冲突。
- `docs/open-questions.md` Q12 对投资者现金池不补外部现金的既有决定，与 simple 分红实际结算的新增许可并列表达；没有写成补充投资者外部资金或保证交易成交。
- 本轮变更范围为文档同步，未发现将仿真实现要求扩入当前 simple 的明显范围漂移。除上述验收缺口外，未见必须阻止本次文档同步的残留矛盾。

## 独立复核边界

本记录只复核上述完整文档和文档 diff；未修改被审查正文、未运行测试，也未复核代码实现。本文记录发现，不表示设计/实现门禁已通过。

## 复核更新（2026-10-06）

复核 Q14 第二章 §1 与实施清单 §6 新增差异后，前述唯一发现已闭合：蓝图现明确开局股价、总股本与虚拟 PE／PB 倍率只用于一次性推导初始利润和权益，再按同期间营收、开支及税务形成一致初值；seed、预览参数与实际创建绑定，后续结算/恢复不重校准且不保证开盘无涨跌。未完成清单项进一步要求金额与期间一致性、预览和创建使用同份 seed／参数、编辑保留、恢复不重复校准的短测。具体虚拟倍率值仍留作显式配置，不冒充真实统计或现行 A 股估值规则。此前“尚未进入工程验收”的 finding 关闭；未见新增冲突。

### 实施状态更新复核

按委托只核对了 `implementation-checklist.md` 当前开头三段及 checkbox 状态，不重审其余正文。当前已实现范围包括 identity/owner 拆分、四周期汇总生成、新局 seed/价格初始化、严格模式状态及财务/披露消费者。最新 Session candidate 在显式登记名册、法定事实和获批现金方案下，已有真实税前账户到账、Simple 应付清偿、同日多方案现金除息锚、失败原子性及恢复/重复调用幂等的定向证据；但 `TreatmentNotConfigured` 表明尚未税后结清，公告/NPC 获知、实际股息税收缴、宿主/UI 入口和其他股本行为未完成，`cash_settlement=false` 保持诚实。集团／冲击旧 Session 接线及完整回归仍留后续门禁。没有把底层登记、税或候选付款能力扩称为共同股本行为完整可用。

Finance 18/18、Session Simple 14/14、typegen 135/135、开局 seed/价格校准 14/14、Web cross-domain 40/40、正确 cwd 的 market fixture 63/63 与 app/node TypeScript 检查通过；既有 Rust 85 项、Web 45 项、默认 features workspace 检查、WASM/Web release 构建、真实存档生成/恢复/续行也按各自命令范围报告。当前勾选与未完成边界一致：实际投资者股本结算与共同公司行为全链、报表完整勾稽、披露期间汇总及自然日日结完整原子链仍未勾，未见过度核销。以上证据由 root 提供，本 reviewer 未重跑构建或短测。

handoff 已准确记录 Web fixture 合同限制：`.tmp/company-system/session-actions/legacy-fixture-contract-limit.log` 中 `save-schema-contract.test.ts` 为 32 项 2 通过、30 失败，原因是旧手工 `currentSaveFixture` 缺 strict 必填 `company_system`；正式 Engine 生成存档 gold 通过不能替代该套件，需迁移 fixture 后按原断言重跑，不加兼容默认或弱化断言。之前的 `protocol-anchor-final.log` 已被重跑覆盖，不再引用其中 46 项混合结果；当前 `protocol-anchor-verified.log` 为 14/14。handoff 对两项分别报告，没有混淆；该限制准确。
