# 公司系统 Web 与宿主接线

本批以当前 `main` 已合入的 `feat/ui-design` 界面为基础，保留 Desktop／Mobile 组织、双向交易、市场控制授权与账户身份隔离。用户的更新设计要求 Simple 也提供完整共同财务与股本功能，随后进一步取消 Simple 的真实公司资金及 `SyntheticFunding`；Simple 使用纯账面汇总，同股本行为不因账面现金不足拒绝。旧的“Simple 没有三表或公司行为”的中间方案已撤回，不能用其测试结果证明现行目标完成。

## 共同入口

三种宿主的新局均将完整 `SessionSetup` 交给 `ProtocolSession::new`，无需再建另一套公司创建端点。WASM 的 `SessionRegistry::create`、Tauri 的 `create_session`、Server 的 `new_session_bound` 不逐字段裁剪公司配置。Web 的 Worker、Tauri invoke、Remote 创建及市场重置同样传递整个 setup。未知模式与尚未实现的 Simulation 由共同配置校验拒绝，不降级为 Simple。

公开查询继续走现有报告页与报告编号查询；玩家只消费公开材料，NPC 仍通过本人已获知集合，不新增可窥读当前公司私有状态的 Host API。财报渲染复用共同四表与更正 UI，来源单独显示为 `SimpleGenerated`／`SimulationAccounting`。

## 新局与恢复

`companySystemDraft` 是新局编辑草稿，不是当前市场状态。它在取得替换门禁前显式解析；非法 JSON、缺字段、额外字段或不支持的模式不能触发新槽、市场重置或取消当前写档。合法草稿进入下一局 `company_system`，Remote 使用同样的配置且不创建浏览器本地槽。seed 在预览之前生成，创建使用同一个已展示的值，不二次抽取；用户 JSON 编辑转为 custom 后，seed 输入不能自动覆盖其金额或配置。明确重新生成保留合法趋势／税务配置，替换 seed 与初始化基准。

明确换档、宿主启动恢复及 Remote 时间线更新从实际 setup 更新草稿；沿用既有 generation／主体校验，不从草稿赋予控制授权，也不改变玩家账户资金。成功日終状态严格恢复选中实现，不接受旧 `groups`、`company_operations`、`closing_registry`、`ops_wiring` 顶层字段，不增 schema 版本或迁移逻辑。

## 当前证据与待完成

通用新局传递专项 3 个短测试已经通过：非法草稿无副作用、本地下一局传递编辑参数且不改默认模板、Remote 重置传递且不建本地槽。case timeout 与外部进程树 deadline 均为 10000ms，并发 4；没有运行复杂回归。该证据只证明传输与副作用门禁，不证明仍在重写的最终 Simple 财务算法。

期间配置、生成状态、变化解释及财务配置／状态 parser 已分别对照 Rust exact DTO，旧月度代际字段和解析入口已移除，不建立别名。四种自然结算周期、独立年化趋势及自然月份进度保留；变量开支规则互斥，收入费用非负、成本率高于 100% 合法，复业来源／金额成对必填，规范 u64 随机与事件游标保真。财务 parser 复用唯一 `Books`、`Closing`、`JournalLine`、`TaxPolicy` 与 `IncomeTaxPosition`。期初开账日必须对应前史开始前一天，完整历史按周期和 Rust UTF8 公司顺序覆盖，最新生成金额与历史一致。金额校验使用与 Rust 一致的精确十二次复利根、十亿倍定点因子与半偶舍入，不以浮点近似接受脏档。

每公司配置的 `kind` 为必填项，显式选择 Industrial／Bank／Insurance／RealEstate，不根据股票代码推断或默认补工业。恢复要求配置、发行人身份及财务状态的类别完全一致；Simple 财务科目表必须是该类别现有完整基础表与五个独立虚拟汇总科目的精确集合，名称、会计要素及现金／备抵标志均不得漂移。汇总应收、应付、营业收入、固定费用及变动费用不冒充贷款、吸收存款、保费或赔付；初始化权益使用 `simple_receivable` 对应实收资本，避免跨类别误用应收保费或缺失的工业科目。该根修定向 27 项短测通过，证据为 `.tmp/company-system/web-kind-final-contract-green.log`。根任务使用 fresh70 正规生成 bindings 与两份真实存档后，同一短命令复验完整存档／日终 15 项、类别与 seed／新局 27 项、初值 helper 3 项，共 45/45 通过，证据为 `.tmp/company-system/web-final-kind-fresh-green.log`；并发 4、case 与外部命令 deadline 均 10000ms。该证据不包含完整回归或全项目 TypeScript 检查。

用户允许虚拟预设且参数可编辑，并允许在初始化时从开局价格反推基本面，避免无解释的规模偏离，不承诺开盘没有跳变。预览 seed 和证券标识确定虚拟 PE（整数 10–30）、PB（1–3）；期初市值由价格乘总股本形成，按虚拟 20% 税前盈余率、25% 所得税参数和所选周期长度倒出收入基准，再形成固定开支与权益。只设置收入、开支及平衡财务期初，不直接保存目标净利润或共同正确股价；初始权益以账面应收对应实收资本，不建真实公司资金。后续基本面仅由年化趋势、环境和独立期间扰动演化，不再读成交价格反推。

默认营收年化区间依目录顺序为 2–10%、4–12%、-8–0%、-12–-4%、-15–-7%，固定开支为 1–4%、2–5%、0–3%、1–4%、0–3%；趋势持续 6–24 自然月。变动开支比例预设 60%；环境初值零、持续系数 70%，各周期噪声独立配置，不冒充真实波动率换算。默认自然月结算、36 个同长度前史期间；切换季度／半年／年预览保留 seed，并重新生成同长度金额基准。本人编辑的周期不自动改写其金额，明确提示同时编辑或重新生成预设。税务为游戏政策参数：增值税销项／进项 13%、抵扣比例 100%、所得税 25%、亏损结转 5 年，不声称适用所有现实公司。

逐公司股本行为偏好仍待共同行为契约接入；长结算周期中更短报告窗口缺少实际生成事实时明确不可用，不平均分摊全年金额或提前公开未来结果。最终生产、生成 bindings 与真实完整 fixture 的根任务门禁尚未完成，不用局部短测替代整个 checklist 完成证据。

完整存档测试已改用 `company_system` 的发行人、汇总账簿、结算记录、生成金额及环境 RNG，原库存结构与四行业业务参数拒绝断言保留在独立低层 parser 上，不将 Simulation 状态塞入 Simple 存档。日终测试直接使用真实已结束日状态；休市日正例要求根任务实际生成的 `2026-01-01` 结算、`tick = 0`、当前自然日为次日及同步公司推进日，不再手改时钟假造结算。原活动委托、冻结资源、未处理输入和父单拒绝断言保留，另检查公司推进日错位。真实主存档及休市日档正规生成后，完整存档／日终两份测试 15/15 通过，证据为 `.tmp/company-system/simple-save-contract-fresh-green.log`；旧 fixture 缺字段及日期预期不匹配的红证据仍保留，不篡改历史结果。

公司低层 parser 测试不再把 `current-company-slice.json` 的 CompanySystem 误作 CompanyOperations；独立 TypeScript fixture 明确构造平衡开账凭证、现行税务 owner、公司身份、业务参数、调度和冲击状态，不声明它是 Rust Session 恢复档。五份定向低层测试 22/22 通过，证据为 `.tmp/company-system/independent-finance-contracts.log`。旧 groups 和 company_operations 配置的有效守卫移至其独立 parser，完整槽则按现行 Simple 契约显式拒绝旧字段；财报样例补齐必填 SimulationAccounting 来源，不新增兼容。

真实 fixture 与 generated bindings 由根任务正规生成，不能手补 JSON 或 generated 类型。Core 最终 CompanySystem 状态、日结装配及股本行为还需同步并独立复核；本记录不宣称完整闭环完成。
