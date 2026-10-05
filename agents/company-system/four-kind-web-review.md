# Web Four-kind 独立复核

## 范围与结论

复核 `simple-chart.ts`、`simple-finance.ts` 及相邻测试、`system.ts`、`system-config.ts`、默认配置与初始化预设、公司配置命令测试，以及 Rust 四行业基础科目表和银行／保险／地产报告科目映射。结论：Web Four-kind 增量在所审契约范围内通过，并已由真实存档相关短测复验；不代表完整仓库验收。

## 契约核对

- `parseSimpleFinanceState` 严格解析四种 literal kind，并按 kind 校验 `Books.chart`。`validateSimpleChart` 核对版本、完整科目 key 集合及每项的名称、`AccountElement`、现金和备抵标志；没有兼容字段或默认 kind。四种版本分别为 Industrial 2、Bank 3、Insurance 4、RealEstate 5。
- 四类映射与 Rust `industrial_account_chart`、`bank_account_chart`、`insurance_account_chart`、`real_estate_account_chart` 的底层科目一致，后三者也与 `accounting/reports/{bank,insurance,real_estate}.rs` 的 codes 和名称映射一致。五项 `simple_*` 扩展只表达汇总应收、应付、营业收入、固定费用、变动费用，不借用银行利息收入或保险服务收入科目。
- `system.ts` 要求系统配置公司与 issuer 一一对应，配置 kind 等于 issuer kind，并继续要求 `finance.kind` 等于 issuer kind；`finance.company`、严格财务配置及图表也经交叉核验。这样 `config.kind == issuer.kind == finance.kind` 均成立。
- 最新增量在 `system.test.ts` 对四种明确配置分别构造完整状态，并调用 `validateCompanySystemSession` 作正例；同时验证配置缺少 `kind` 与非法 `Other` 均被拒绝，且不按股票代码推断类别。
- 期初行校验要求科目来自该 kind 的精确科目表且不得为 Revenue／Expense。默认配置与价格锚定预设使用 `simple_receivable` 借方、`4001` 实收资本贷方，符合简单账面模型，不声称是真实现金流。默认公司仍显式为 Industrial。
- ADR-0035 的“收入及费用汇总、账面展示不模拟真实现金流”边界与上述科目、初始化一致。所审改动不涉及沪深撮合或交易制度，因此不需要额外交易所规则依据。

## 测试证据与限制

- `.tmp/company-system/web-kind-final-contract-green.log` 记录更新后的指定 Web Four-kind 合同短测 27/27 通过；`web-kind-contract-red.log` 是实现前红测记录，包含缺少 `kind` 的预期失败。
- 新增边界覆盖四 kind 精确 chart、版本/缺科目/定义漂移、完整公司系统与 Session 正例、错误类别绑定、缺字段与额外字段、期初收入费用科目拒绝及预设账户配置。
- `.tmp/company-system/web-final-kind-fresh-green.log` 覆盖真实存档短测 15 项、Four-kind／seed／新局 27 项及 preset helper 3 项，共 45/45 通过（并发 4、case 与外部 deadline 均 10 秒）。
- 亲读 `current-schema-save.json` 与 `current-closed-day-save.json`：前者五家公司、后者一家公司，setup 与保存财务状态类别均为 `Industrial`；chart version 均为 2，且精确含五个 `simple_*` key。两份档的自定义期初行使用 `1122` 应收账款与 `4001` 实收资本，均是该 Industrial 科目表中的合法非收入／费用科目；这不改变默认配置和价格锚定预设使用 `simple_receivable` 的结论。
- `web-host-integration.md` 与 `web-simple-source-handoff.md` 已注明 fresh70 后真实 bindings 与两份存档的 45 项联合证据；此前旧 15/15 日终／存档证据不单独覆盖新类别合同。
- 另复核 `simple-finance.test.ts` 的 TypeScript 可变性修正：missing／drift case 对 `chart.accounts` 建立显式可变拷贝，再删除或替换 `simple_revenue`；两处既有拒绝断言及 `/simple_revenue/` 匹配保持不变，无断言绕过。`.tmp/company-system/web-kind-test-types-green.log` 记录该文件 4/4 短测通过（并发 4、case 与外部 deadline 均 10 秒）。
- 未运行 Cargo、tsc 或完整回归；未声称跨宿主或完整项目验收通过。证据限于上述短命令和两份真实 fixture。

## 发现

本轮未发现需要修复的有效问题。
