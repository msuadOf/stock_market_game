# Simple Web 独立复核记录

复核人未参与本批实现。已全文阅读 676 行的 Q14 设计稿，后续作者转达用户再次取消真实公司资金和 `SyntheticFunding`：现行 Finance exact 使用应收收入／应付开支的账面汇总，不建立真实公司现金。Simple 与 Simulation 对上层提供同一财务及股本行为契约，Simple 按月生成营收和开支，再推导利润。此前按能力缺省财报及结算的方案已经取消，下面保留的初审与 18 个旧合同短测不作为现行方案的完成证据。

现行 UI 已撤回 `SimpleFundamentalsReport` 与 `CompanyPanel` 模式分叉；`ReportNotes` 仅增加公开材料来源标签，共用四表与更正入口。新局 editor 已改为营收／开支驱动和无真实现金的财务汇总。严格完整 state／公共 report parser 仍等待作者与 Core 最终合同，当前不标记通过整批最终门禁，不复核已失效的旧净利率公式模块。

## 现行 Finance 配置及局部状态复核

完整检查 `simple-finance-config.ts`、`simple-finance.ts`、对应测试、更新的 `system-config.ts`／config 测试、`defaults.ts` 和 editor 文案；对照 `finance_config.rs`、`finance.rs` 字段，以及唯一 `JournalEntry.validate_invariants` 与 `TaxPolicy.validate`。局部结论通过，没有阻塞发现：

- 期初凭证必须含正金额且借贷平衡，规范金额及累加总额保持 i128 范围，税务范围与共同政策校验一致。
- `generation`／`finance` 分别嵌入新公司配置；旧 SyntheticFunding、净利率、公开延迟等未知字段不兼容接受。JournalLine／TaxPolicy 仅 export 已有 parser 复用，没有复制实现或更改既有行为。
- 默认五个公司参数和 editor 列示数值对应；期初应收借方对应资本贷方、无现金，明确标为虚拟汇总而非具体客户欠款。税率及期限明确为可编辑游戏政策，不冒称全部现实公司的统一法定待遇。
- 局部 Finance state 保留 Books 与 Closing，不附加独立现金；规范正 u64 事件游标、年月一致与亏损池年份顺序检查合理，但尚需作者最终 Core restore 合同确定与整体接线。

独立运行 Finance、公司配置和月度生成三个短 suite，10 个 case 全部通过，247ms，case／外部进程树 deadline 均为 10000ms，并发 3。没有运行 Cargo 或复杂回归。当前 Core 月结和完整状态仍在开发；parseBooks／Closing 的复用不构成对新整体恢复语义的证明，不关闭整批门禁。

Journal 增量已复核：唯一 `BusinessKind::SimpleMonthlySummary` 与当前 Rust 枚举一致；Web 要求对应 `cash_flow` 精确为 `NonCash`，不借用客户交易或资金生成标识，不改变其他既有分录。独立 Finance 短测 3/3 通过，229ms，case／外部 deadline 10000ms；`git diff --check` 无错误。局部 Web guard 通过，但当前读取的 Rust 唯有枚举新增、尚未有相同 guard；作者已要求 Finance 实施者补齐生产过账／restore 校验，完成后才能声明跨层闭合。

后续 `opening_date` 必填字段增量已复核，与 Rust 新字段对应，独立验证自然日合法性并拒绝晚于 `as_of`，不从更正或报表版本反推开账历史；缺字段测试循环包括它。摘要负控现已遍历 Operating／Investing／Financing 三类，保留合法 NonCash 控制。增量没有阻塞发现；独立 Finance 3 个 case 再次通过，case／外部 deadline 10000ms，并发 2。整体 state 接线和 Core 同等恢复约束仍待完成。

## 现行月度参数 parser 独立复核

已完整检查 `monthly-generation.ts`、`monthly-generation.test.ts` 与 `simple-values.ts`，逐字段对照 Rust `company/simple/monthly.rs` 的 `MonthlyGenerationParameters`、`VariableExpenseRule` 和 `validate`。本局部结论通过，没有阻塞发现：

- 营收及固定开支分别配置，不包含独立净利润或净利率驱动；旧 `net_margin_bp` 被 exact shape 拒绝。
- `RevenueRatio` 与 `Growth` 的标签及字段互斥；成本率超过 100% 合法，比例扰动区间必须非负且保持 i32 可表示。
- 增长率最低 -10000 bp、扰动非负、敏感度允许完整 i32，与 Core 校验一致；非法数值、缺失及未知字段不补默认。
- AccountingAmount 保留规范两位小数元字符串和完整 i128 分内部范围，拒绝负零、非规范格式、数值输入及溢出。非负营业金额额外拒绝负数。
- 不增加交易、税务或资金授权规则；参数解析属于共同保存边界，没有引入新依赖、兼容转换或内部模式耦合。

独立运行作者的 3 个新 case，全部通过，总耗时 213ms；case timeout 和外部进程树 deadline 均为 10000ms，声明并发 2。另独立执行 i128 上下界、越界、规范金额及 i32 两端与非法数值检查，全部通过。没有运行 Cargo 或回归。该证据只覆盖月度参数解析，不能证明尚在完成中的 Core 月度生成、完整 config／state、资金事件或宿主闭环。

月度保存 DTO 后续增量也已独立复核：`MonthlyAmounts`、`RevenueExpenseState`、`ExpenseChangeExplanation` 的 exact 字段逐项与 Core 同名结构一致；金额非负、RNG 规范 u64 字符串（包含最大值）、复业两个 required nullable 字段成对且来源非空／金额为正。解释中的实际扰动保留正负 i32，而配置中的扰动幅度仍要求非负，未混用两者。拒绝未知 profit 字段，不保存另一套权威利润。没有阻塞发现。

独立运行更新后的 5 个短 case，全部通过，205ms，case／外部进程树 deadline 10000ms，并发 2；另外逐个删除解释字段、检查合法复业及实际负扰动、复业配对／空来源、RNG 越界及非规范字符串，均符合预期。该局部 parser 尚未接入 final slot 整体状态，不据此关闭严格恢复或整批门禁。

## 现行共同附注与新局参数传递复核

完整检查 `ReportNotes` 的 source-only 标签及新文案、`public-financials-render.test.ts` 新 case、`company-config-commands.test.ts` 三个 case 和所对应的 `useSaveCommands` 公司草稿解析／传递 hunk。局部结论通过，没有阻塞发现：生成来源不改变四表、附注明细或更正入口；删除旧股东分配禁用文案符合最新设计，不声称完整真实会计准则合规。非法草稿在替换与清理前拒绝，合法编辑配置按值进入新 setup、不修改默认模板，Remote 重置不创建浏览器槽且保留现有控制与账户分离。

独立运行上述两个 suite，6 个 case 全部通过，637ms，外部进程树与 case deadline 均为 10000ms，并发 2。新 source case 证明共用附注渲染入口，不证明股本动作已完整实现；新局 tests 使用当时的默认配置，只证明参数传递门禁，不为最终 Finance config／state 结构提供验收证据。没有运行 Cargo 或复杂回归。

## 整体 CompanySystemState 首轮复核发现

完整阅读新的 `system.ts`／test 和 Core `simple/state.rs`、`api.rs`、`monthly.rs` 生成与恢复校验、`CompanySystem` 自定义 Deserialize、发行人集合校验。首轮发现以下五项跨层差异，现已修复并通过再次独立复核：

1. 营收增长的 JS 三项总和未保留 Core 逐步 i32 checked-add 顺序；中间溢出、最后被扰动抵消时误接受。独立最小 parser fixture 已复现 `2147473647 + 10001 - 10001` 被接受，但 Rust 第一步已越界。
2. 有复业时 JS 直接使用复业金额，跳过 Core 覆盖前必做的原营收 grow 校验，可能接受非法增长因子。
3. JS BigInt 基点乘法未检查 Core `AccountingAmount.bp_product` 的 i128 中间乘积范围；商仍可表示不能掩盖乘积溢出。
4. Web 发行人仅逐项校验，缺少 Core `IssuerRegistry.new` 的重复上市股票、未知母公司及集团环检查。独立最小 parser fixture 已复现未知 `group_parent` 被接受。
5. 历史公司排序使用 JS UTF-16 比较，与 Rust String 的 UTF-8 字节顺序不一致；合法 U+E000 与 U+10000 公司 ID 的顺序会反转，导致合法 Native 月史误拒。

作者完成逐步 checked i32 加法、先 grow 再覆盖复业、i128 中间乘积 guard、发行人集合校验与 UTF-8 字节排序。再次完整读取增量确认五项修复符合 Core 当前规则，不弱化断言，也不改变半偶舍入。新增短负控包含溢出抵消、复业非法因子、乘积溢出、未知母公司／环／重复映射；Unicode 双公司合法顺序控制及反序负控均有覆盖。

独立运行 `system.test.ts` 与月度生成 parser suite，11 个 case 全部通过，250ms，并发 2，case／外部进程树 deadline 10000ms。本轮旧 guard 发现已全部关闭，当前状态 DTO 局部复核通过。年度／长期趋势的新合同与真实完整 slot 尚在实施，不以这些新范围未完成倒推现有 guard 错误，也不把局部 parser 通过扩大为整个目标完成。没有运行 Cargo 或复杂回归。

Session 接线增量复核通过：`validateCompanySystemSession` 对照 Rust `validate_company_domain`／`issuer_specs`，确认配置相等、公司推进至自然日时钟前一日、发行人集合与 setup 股票一一对应，以及重建名称、行业、类型、证券代码、总股本和无母公司条件一致；root 在解析公司系统、setup 与时钟之后实际调用。独立运行更新后的 `system.test.ts`，7 个 case 全部通过，275ms，包含合法 Session 控制、错推进日和修改股本负控；case／外部 deadline 10000ms，并发 2。raw company state 的 Unicode 公司用例与完整 Session 的固定发行人映射边界分开，不互相冒充。此证据关闭该 helper 接线局部门禁，真实整体 fixture、年度／趋势与其余公司领域关联校验仍须各自完成。

## 已取消方案的初审范围与依据

完整阅读所复核的 `SimpleFundamentalsReport`、`CompanySystemInput`、`company-system-config`、对应新测试，以及 `CompanyPanel`、`ReportNotes`、`App`、`useSaveCommands`、`useSessionHostLifecycle` 的公司系统相关 diff；对照 Q14 公司系统设计蓝图和当前 Rust `SimpleConfig`。本批不新增 A 股交易制度，而是明确标示虚拟简化基本面，不将其冒充真实会计报表。

## 已取消方案的初审结果

- Simple 展示保留精确元字符串与负净利润；明确区分零权益与不支持权益，不虚构现金、客户、三表或结算。
- Accounting 报表入口保留既有 UI 组织；更正界面限定 Accounting 材料，没有向 Simple 暴露空三表更正。
- 新局草稿经显式解析进入 setup，错误显示给用户；局内没有模式切换。恢复同步草稿的 hunk 未改变既有 generation、账户和控制授权门禁。
- 当前 `publication_delay_days` 仍与 Rust 临时合同一致；若父层采用唯一 `ReportFrequency`，必须同步删除配置、说明和测试，不能形成两个公开时钟。
- Quarter 可表示季度内截至某月末的窗口，与现行 Rust 报告规则一致；UI 明确显示起止日，本批不擅自把它改为只能季度末。

## 已取消方案的简单测试

独立运行 `company-system-config.test.ts` 和 `simple-fundamentals-render.test.ts`，并发 2，case timeout 10000ms，外部进程树 deadline 10000ms。5 个 case 全部通过，Node 总耗时 567.8ms。没有运行 Cargo、完整回归或浏览器复杂验收。

## 现行方案待最终复核

公开报告 parser 与 company state parser 尚在作者完成过程中，需要核完整严格合同、公开与私有边界、来源／单位、日期、能力枚举和恢复结构。建议补短测固定非法草稿无副作用、合法草稿传递到 local／Remote 新局、成功恢复同步草稿及迟到恢复不能覆盖新时间线；现有 ports 补齐本身不证明这些行为。
