# Q23 更正事实 Web parser 收口

- root 授权补 `report_correction_operations` 当前存档必填字段，复用 Q23 Engine 冻结 DTO，不设版本、不补旧字段、无迁移。工作源码位于 `apps/web/src/save/schema/company/report-corrections.ts`，由 `schema/root.ts` 受控入口接线。
- 导出 `CompanyReportCorrection`／`CompletedReportCorrection` 与 request／completed／map 三个严格 parser，供 UI 作者的实际 host API 接线复用；`parseJournalEntry` 仅新增导出，没有修改其他会计解析规则。
- `BusinessEventId` 为安全整数 number，`PublicationId` 为 u32 number，`AccountingAmount` 仍为独立 i128 会计**元**字符串，`CivilInstant` 为既有 date／second_of_day 对象。复式凭证检查非空、每行正金额、借贷双方和相等总额；比较 owner journal 时按精确会计 minor units 规范事实键，保留返回原元文本，不把 `Money` i64 分编码套入会计金额。
- 结构校验包括 operation 键／请求身份一致、非空原因／凭证／结果、公开 ID 全局唯一及未知字段拒绝。存档关联校验覆盖已结束自然日、`Industrial`／`Bank`／`Insurance`／`RealEstate` 实际 `TaxOwner` 的 journal source／有效期间、原目标为该公司的 `Standalone` 且在更正时点已公开、真实同日终 `Correction` 结果、公司或实际 group scope，以及同原版 scope／period／supersedes 的直接关联。级联结果允许原期间及之后已公开期间，不误拒绝后续报表更正；不以 `Consolidated` 原目标冒充单体 owner 更正。
- 五个普通短 case 通过，外部 10000ms 进程树 deadline、Node case 10000ms、并发 4。覆盖畸形结构、不平衡／非正金额、owner／期别／scope／公开链、等值会计元文本及合法对象键；首次红仅是新 parser module 未存在，不声称先取得可编译业务红。
- 非作者 `review_q08_final` 指出原字面 JSON 金额比较会拒绝 Rust 可接受的等值元文本，已修并复核。Engine directlink scope／period 与 Web 已同步；Q23 作者再指出原目标必须是 `Standalone`，也已修并增设同公司／同期间 `Consolidated` 目标绕过的拒绝 case。最终非作者精准重核通过，无剩余阻断。root 统一实际 fixture 重生成及完整类型检查，未手写 generated 或旧 JSON。

## 四类 TaxOwner 严格契约增量

- 三类新增账套必填 `income_tax_policy`／`income_tax_position`，`Industrial` 仍保持 `tax_policy.income_tax`。从原 Industrial shape parser 抽出 `books/income-tax.ts`：四类复用同一 shape、年度 FIFO／到期／半偶计算与 GL 守卫，不复制三套算法；缺字段或未知结构直接拒绝，无格式版本、旧字段补齐或迁移。
- 年度状态检查亏损池起源年递增／正金额、年初与上年年末连续、当期税额／递延税资产重算、最后亏损池一致及精确 i128 中间运算边界。Books 检查真实 `OpeningBalance` 的 DTA、最新 GL DTA、累计应交所得税计提、三税科目的要素／现金／备抵语义、纯税务 `NonCash` 计提、`Operating` 缴税只能 Dr 应交所得税／Cr 真实现金，以及 restatement 来源真实存在且有效期间早于实际过账期间。
- `accounting/amount.ts` 共用精确会计元比较键，遵循 Engine 文本解码的幅值上界，连最负 i128 文本的非对称解码边界也严格对齐；不变更 `Money` 分编码、不归一化返回原会计元文本。
- 实现前十二个实际 parser case 为 **1 绿 11 红**：Industrial 旧 shape 接受负税与不连续年度链，另三类不认识必填新字段。实现后包括新增代表性守卫的 **23/23 短测通过**。日志 `.tmp/checklist-wave4/q23-web-tax-owner-red.log` 与 `.tmp/checklist-wave4/q23-web-tax-owner-green-review.log` 保留；Node case 10000ms、命令进程树 10000ms、并发 4。合成 fixture 使用显式 2500 基点／5 年，仅验证现行游戏公式，不声明生产默认政策或真实税法参数。
- 新边界包含实际 half-even tie、结转第 5／6 年、两个亏损年 FIFO、基点乘积溢出、全部三税科目 cash／contra／element、伪税源、退款／非现金抵税／错误现金流、unknown source／同月或未来有效期；更正关联分别验证四类 owner 的正常与来源缺失分支。非作者复核先指出上述测试缺口，补齐后复查代码和原日志无剩余阻断。
- UI 控制区明确四类实际 `TaxOwner` 与结构化子账限制，包括保险合同组及主营保险损益；不能把 schema 层或四种 presentation 等同于四类真实经营、更正／税额／公开版本的端到端验证。root 后续统一真实 fixture、Rust 通用链与完整类型检查，本增量不宣称跨层整体已核销。
- 既有 `company-books-schema.test.ts` 的三类 source toy mock 补当前必填税务事实及科目，复用 `tax-owner-test-fixture.ts` 中显式合成数据；其年度成功 case 现在使用对应 `TaxAccrual` GL 的合法 toy `Industrial`，不把修改后的年度 position 与旧真实 company GL 随意拼接。原成功 deepEqual 和全部拒绝断言保留，五个相关 source case 短测通过，日志 `q23-existing-books-source-green.log`。三个读取真实 Industrial slice 的旧 fixture 缺年度 assessments，仍交 root 从真实 producer 重生成，没有手改 JSON 或建立 fallback。
