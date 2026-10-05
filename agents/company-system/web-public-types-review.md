# Web 公开类型修复独立复核

日期：2026-10-06。复核者未参与本批实现。范围限于 `apps/web/src/save/schema/company/system.ts`、`apps/web/src/app/company-config-commands.test.ts`、`apps/web/src/app/startup-recovery.test.ts` 及其 generated 类型关系；未修改源码，未运行 Cargo、完整 TypeScript 检查或完整回归。

依据：`AGENTS.md`、`docs/principles.md`，以及 `.tmp/checklist-wave4/host66-web-types.log` 与 `.tmp/company-system/web-public-types-green.log`。前一日志是修复前记录，包含公开 `CompanySystem`／`CompanySystemConfig` 缺失和测试推导／mock 泛型错误；后一日志是定向测试记录，22/22 通过，记录包含每 case 10000ms timeout、并发 4。它不能证明完整 TypeScript 检查通过，本复核也没有作此结论。

静态复核结论：`system.ts` 将 `CompanySystem` 定义为 `ReturnType<typeof parseCompanySystemState>`，并从 ts-rs generated `CompanySystemConfig` 重新导出，满足 generated `SaveSlot` 和 `SessionSetup` 指向该模块的既有契约；没有手改 generated 输出。`SessionSetup` 仅以类型导入并用于 session 校验参数，解析器返回类型不依赖 `SessionSetup`，未发现由此形成的 alias/inference 循环。命令测试为 `find` callback 及 `actual` 明确 `SimpleCompanyConfig` 类型。此次改动仅修正公共类型暴露与测试类型表达，没有改变配置解析、存档内容、seed、交易或公司基本面语义；符合大 A 语义，且范围必要、简洁。

增量复核：实现者将 entropy mock 改为 `function<ArrayType extends Exclude<BufferSource, ArrayBuffer>>(array: ArrayType): ArrayType`，普通分支仍调用原方法并返回同一个 `array`。签名与 `Crypto.getRandomValues` 的泛型约束和返回类型一致，先前类型推导 finding 已关闭；无 cast，也未改动运行时行为。其余旧日志中的命令测试隐式 `any` 与 generated 导出错误在当前代码中分别已有显式类型、导出定义。

验证限制：只引用上述真实定向短测记录，完整 TypeScript 检查未由本复核执行或确认通过。

## 存档 fixture 契约增量复核

范围增量：`apps/web/src/save/complete-save-schema.test.ts`、`day-end-archive.test.ts`、`company-slice-test-fixture.ts`、`company-contracts.test.ts`、`company-schema.test.ts`，以及 `agents/company-system/web-host-integration.md`、`web-simple-source-handoff.md` 中对应范围。未改源码，未运行 Cargo 或 TypeScript 检查；按提供的记录核对真实短测结果：完整存档／日终 15/15（`.tmp/company-system/simple-save-contract-fresh-green.log`），独立公司 parser 22/22（`.tmp/company-system/independent-finance-contracts.log`）。

完整存档正例仍经 `parseSaveSlot` round-trip，并用路径复制变异保持共享 fixture 不变；现行 `company_system` 的发行人、Simple 财务账簿／生成状态、环境 RNG 和交叉字段取代旧 Simulation 容器路径。完整槽继续显式拒绝 `company_operations`、`closing_registry`、`ops_wiring`、`groups` 等退休字段。旧公司库存与四行业 flow 的坏值断言由完整槽迁到对应独立 parser：`parseIndustryBooks` 仍拒绝非法 inventory，`parseFlowParams` 对 Industrial、Bank、Insurance、RealEstate 各保留代表性错误路径断言。它们不再伪装成 Simple 完整存档支持 Simulation，范围移动与当前契约一致，未发现断言删除或削弱。

`company-slice-test-fixture.ts` 现在直接构造仅供低层 parser 测试的 CompanyOperations 样例，并在每次调用时 `structuredClone`；测试 fixture 的 OpeningBalance 借贷各 100.00、账簿科目对应，且带明确虚构公司身份、税务 owner、业务 flow、调度、随机状态和冲击。它没有从生产 `CompanySystem` JSON 假扮为 Simulation 恢复状态，也没有接入完整槽或产品资金结算链；该数值是平衡 parser fixture 数据，不构成现实公司资金、真实资金来源或 A 股行为断言。公司配置和 `groups` 正反守卫仍在其各自低层 parser 测试，完整槽只断言旧字段被拒绝。财报 parser 样例补上必需的 `SimulationAccounting` 来源值，未将它冒充 `SimpleGenerated`。

休市日正例改读根任务正规生成的 `current-closed-day-save.json`，精确检查 `2026-01-01` 结算、`tick = 0`、当前日期 `2026-01-02`、公司推进日期同步及 `Closed` 市场覆盖；检查已安装 fixture 字段与测试相符。该测试验证项目默认 `TradingCalendar` 下的日结／存档契约，不把本 fixture 单独当作官方休市日历依据。原未日结、日内状态、连续／竞价委托、冻结资源、过期生命周期、待处理玩家／NPC 请求和未完成父单拒绝断言仍在；NPC 样例从 fixture 原本的 `null` 状态显式构造与当前 tick 对齐的非空请求，再传给日终验证器，拒绝断言仍直接覆盖目标约束。公司推进日期错位和 archive coverage 缺失新增为负例。没有发现真实资产、委托或结算数据被编造进生产存档。

交接文档准确区分了正规生成 fixture 与独立 parser fixture，并保留旧 stale fixture 红记录，不把其失败日志覆盖成成功；当前绿色只按上述两组日志报告，未推导出全量 Web、完整 TypeScript、Rust 或三宿主验收通过。此增量未改变交易制度或单位语义，必要性在于 Simple slot 契约迁移以及保留旧低层 parser 覆盖；静态复核无有效 finding。
