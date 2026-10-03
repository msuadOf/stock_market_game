# 三份公司政策文档生产链复核（luna04）

## 范围、基线与方法

- 复核目录：`.worktree/implementation-reaudit`；按任务给定以产品基线 `08e4fc7`、与 `b76ece3` 产品等价为准。仅审查与新增本记录；未改产品、未写 Git、未跑测试或长验收。
- 开工读取 `AGENTS.md`、`docs/principles.md`；判断时遵循较新 ADR/已接受决策优先，不把静态检查说成运行验收或官方法源复核。
- 三篇连续从首行读至 EOF：`docs/company-accounting.md` 251 行（1–251），`docs/company-actions-design.md` 94 行（1–94），`docs/simulation-calendar.md` 125 行（1–125），合计 470 行。先前一次三文件合并输出受截断影响，随后按独立文件完整读取；上列行数由 `wc -l` 确认。
- 查当前调用链采用 caller → owner → consumer 顺序；旧总账条目在当前源码重新证实。报告中的代码行号为本复核 worktree 行号。

## 逐章覆盖

| 原文范围/承诺 | 当前 caller → owner → consumer 与判定 |
|---|---|
| `company-accounting.md:1–13`：本文件是来源/覆盖登记，不声称全准则覆盖；需机器清单 | 文档指向 `packages/engine/tests/fixtures/company-model/policy-sources.json` 和 `policy_manifest`。本轮未运行 manifest 测试、未联网复核法规，故只能确认文档声明/引用存在，不确认取证日后法规仍有效或 fixture 与每个实现严格同构。 |
| §1 `:14–30`：规则时间与模拟时间分离、早期开局假设 | `SessionSetup` 只携 `simulation_policy_id`，`session.rs:945–961` 限当前 ID；`session/persistence.rs:260–264` 与 `persistence/v2.rs:124–128,237–241` 恢复拒绝别的 ID。自然日日历另有真实冻结：`CivilClockSave` 保存完整 `CalendarPolicySpec`（`session/civil_clock.rs:118–131,452–460`），restore 经 `CalendarPolicy::from_parts` 校验并按档案重建（`:240–244`）。这证明**日历政策已存档冻结**，不等于存在显式、可独立选择/冻结的 `RegulationProfile` 类型；准则适用、税务政策与 simulation ID 的完整身份映射仍是 Q03，不可仅凭文档措辞核销。 |
| §2.1 共同 `:32–61`：共同准则、报表/税务、合并与已知简化 | 合并规则简化、税法取证阻塞等是文档层规则。当前披露 caller `session/disclosures.rs:157–196` 固定 `ScopeId::Standalone`；没有证明共同覆盖矩阵每行均有生产消费者。本表逐项实现并未独立审计全部业务合同/税率；不将文档登记视作生产闭环。 |
| §2.2 工商 `:62–70`：收入、履约、存货阻塞 | 工商经营消费在 `company/operations/industrial.rs:43+`；日常实际生产链见 G35（下文）。CAS 1 原文阻塞仍是文档明确边界，不能当作已按准则实现。 |
| §2.3 银行 `:72–85`：存款、贷款、ECL与边界 | 银行处理器存在于 `company/operations/bank.rs`、利息/到期分发在 `company/operations/dispatch.rs:33–56`；此事实不证明新局可装配银行，G36 仍在。 |
| §2.4 保险 `:87–106`：GMM 支持范围和简化 | 保险处理器存在 `company/operations/insurance.rs`，保险准则简化仍为游戏假设；与四行业生产装配断点分开（G36）。未把官方链接重复当作本轮核验。 |
| §2.5 地产 `:113–126`：预售/交付、CAS 17 阻塞/游戏假设 | 地产业务处理器及地产债务能力存在，但 G36 的会话行业装配缺口、G35 的工商期末缺口分别独立；CAS 17 原文仍受阻。 |
| §2.6 `:128–134`：行业经营节奏游戏假设 | `OperatingDayRun::advance` 的当前顺序是 expire shocks → sample → due dispatch → industry flow → schedule next interest（`company/operations/day.rs:79–85`）；只能证实现有操作顺序，不证明各项经营假设完全兑现。注意 `schedule_next_interest` 是利息**计提到期**，不是偿还商业借款。 |
| §2.7 `:135–154`：结账/合并游戏简化 | 生产披露确由日终 `DisclosureDispatch::run_day_end` 承接（`session/disclosures.rs:99`），但披露发布 scope 仍 standalone（`:178–196`）；集团报表算法存在不替代固定集团生产发布。G28 仍缺。 |
| §2.8 `:156–165`：注意力发现权重假设 | 本轮未扩大到 NPC 注意力实现；这是现行版本化游戏参数声明，不把它改写为真实市场参数。 |
| §3 `:167–173`：解释第20号与不支持工具 | 文档明示复杂金融工具边界；未发现这里给公司行为开放运行时接线。未重新核对官方法源。 |
| §4 `:175–194`：CAS 生效安排 | 这是规则依据陈述；本轮没有官方网页/原文复取，不能确认 2026 后有无新修订。生产调用链亦不能验证这些日期本身。 |
| §5 `:196–211`：披露窗口、自然日游戏排期 | `CivilClock` 以其冻结日历计算日状态并由 session 日终驱动；披露 dispatch 按日期/报告种类调用 `publish_scheduled`，后者生成 standalone report。Q1/Q3期限在文档标为 blocked。本轮没有重新验证披露偏移算法或法定依据。 |
| §6 `:213–232`：UnsupportedContract 必须显式拒绝 | 文档承诺清楚；本轮不逐类构造合同探测运行时拒绝，故只能列为范围边界、不是验证通过。 |
| §7 `:234–251`：受阻取证与解除条件 | `policy-sources.json` 是其证据 owner；阻塞项目仍需原文补证/更新fixture。不能从实现使用固定默认政策推断阻塞已解除。 |
| `company-actions-design.md:1–12`：设计文档、不实现股东结算 | 当前 session 公司披露入口只发布报告，没见分红/股权事件调用。本轮搜索只核当前产品方向，没有穷举所有序列化/API 的负向全集，因此不把“无任何代码路径”扩张成全仓证明。ADR-0024（已接受）支持不要求投资者资金循环，允许现金池减少。 |
| §1 `:13–19`：商业收付属 K2/K3、股东分配冻结 | 商业应收/日常费用链实际经过 `advance_civil_day → OperatingDayRun → industry flow`；工商借款支付闭环具体见 G35。ADR-0024 §决策5明确公司商业收付/借还/税费独立，不自动转入投资者账户；因此不把商业支付遗漏误解为股东分红需求。 |
| §2.1 `:22–37`：商业借款和债股边界 | `dispatch.rs:33–55` 对 Industrial/Bank/RealEstate 只调用 accrue interest；现有还款方法存在不代表日程会调用。支付遗漏已由 G35 覆盖，不重复编号。CAS37/30 设计依据本轮不重验。 |
| §2.2 `:39–52`：利润留存、资本交易与股本 | 当前报告文档与 disclosure 路径仍是报告，不见公司行为执行入口；股本静态开局语义并未被普通撮合改为公司增发。本轮无系统性存档/API 扫描，不对绝对不存在做更强断言。 |
| §2.3 `:54–65`：登记子账未来边界 | `StockSpec` 固定发行股本及流通量属于开户事实；没有看到本次生产日终公司行为写登记子账。与本发布明确不支持一致。 |
| §2.4 `:67–76`：投资者层税费排除 | ADR-0024 不要求资金循环，但不授权添加红利税/分配；公司所得税与增值税属于公司会计，仍落在 G35 当前工商闭环未完成范围。 |
| §3 `:78–85`：除权除息需未来补规则 | 不支持除权参考价与本发布股东结算边界相符；本轮未重新核验 `trading-rules.md` 官方依据。 |
| §4 `:88–94`：无分红/融资/回购/队列/API | 当前生产披露链发布财务报告；没有看到这些行为进入披露日程。只是只读静态追踪，不作为对所有 API 的穷尽负向审计。 |
| `simulation-calendar.md:1–10`：K1 范围、engine owner | `CivilClock` 持有 `TradingCalendar`，保存/恢复 calendar spec；日状态由 `calendar/holidays.rs` owner 计算，CivilClock 在推进、日结等点调用。 |
| §1 `:11–23`：公历算法、交易时段 | 本轮未复核日期算法全部边界测试；交易日状态生产算法见 G15。材料明确自然日不伪造交易分钟；没有观察到公司日终链改变市场 tick。 |
| §2 `:25–32`：默认/开局/上界 | `CalendarPolicy::default_v1` 定义默认2030、运行2000–2099、初始化1998（`calendar/policy/mod.rs:59–74`）；CivilClock new/restore 验运行起点（`:232–244`）。实际越界/非交易日期用例没有运行。 |
| §3.1 `:36–48`：2026 官方覆盖受阻、模拟回退 | 当前默认 `official_coverage: Vec::new()`（`policy/mod.rs:64`），所以默认并无 Official 覆盖。对自定义含覆盖 policy 的优先级错误见 G15；不能把 `2026` 数据标签当官方核验。 |
| §3.2 `:50–55`：历史覆盖缺证、SimulatedHistorical | `CalendarPolicySpec` 保存覆盖和回退事实；restore 使用存档 spec。官方年度事实是否完整仍是外部证据债。 |
| §3.3 `:57–68`：模拟假日游戏规则 | `simulated_holiday_kind` 运行春节、元旦、劳动节、国庆、清明/端午/中秋回退（`holidays.rs:95+`）；在官方覆盖年仍调用回退是 G15，而非节假日算法或农历来源问题。 |
| §3.4 `:70–79`：状态标签须区分并存档 | CalendarPolicySpec 中覆盖来源、模拟回退及facts被CivilClockSave序列化；状态标签由 `DayStatus/ClosedReason` 返回。冻结日历事实已实现，不核销 G15 决策优先级问题。 |
| §4 `:81–89`：双时钟、自然日事项与休市推进 | CivilClock 的 `day_status`/推进调用日历（`civil_clock.rs:320,342,369,439`），自然日日终宿主再跑公司经营 `session/company_operations.rs:76–86`。这建立 caller→owner→consumer 链；本轮未逐案验证全部 due sequencing。 |
| §5 `:91–105`：RegulationProfile、早期开局假设 | 日历 policy 冻结是可证的（CivilClockSave）；准则规则档案/税务/披露规则尚未证明同被该日历快照承载。静态 `simulation_policy_id` 只识别当前 ID，不是完整政策内容，Q03 保持开放。此处文档自身须与实现后续收口，但本轮不改产品/正式文档。 |
| §6 `:107–114`：ACT/365F 游戏合同日历/计息 | 工商合同 maturity 由 `OperatingScheduler` 以 CivilDate 派发；滚动利息日程存在。日程计息与还本付息缺口区分见 G35。未执行跨周末短算例。 |
| §7 `:116–125`：报告自然日排期及法定窗口 | `DisclosureDispatch` 的日终 caller 到 `publish_scheduled`，按 Period/ReportKind 生成公告。组报缺口见 G28；Q1/Q3 的法定窗口原文继续 blocked。偏移算法/周末披露未跑测试。 |

## 旧总账结论的重新证实

| ID | 本轮状态 | 独立当前证据与限定 |
|---|---|---|
| G15 | 仍缺 | 日历状态 caller（CivilClock `civil_clock.rs:320,342,369,439`）→ owner `calendar/holidays.rs:72–91`。owner先周末，再只在官方覆盖 `entry.covers(date)` 时返回 OfficialHoliday；若当年覆盖存在但目标日期非覆盖日，仍继续调用 `simulated_holiday_kind` 并能返回 SimulatedHoliday。这和文档 `simulation-calendar.md:39–48,59–68,70–79` 声称“真实公告按原文日期执行/官方覆盖替代模拟结果”存在策略优先级差。默认 policy 没官方年覆盖（`calendar/policy/mod.rs:64`），所以不声称默认局触发。 |
| G28 | 仍缺 | caller `DisclosureDispatch::run_day_end`（`session/disclosures.rs:99+`）→ `publish_scheduled`（`:157–196`）→ `PublicLibrary::publish_closed`；request scope 恒 `ScopeId::Standalone(member)`（`:186`）。closing snapshot owner 仍只创建 standalone；Consolidated 算法/报表生成器不能证明本生产发布链有合并版本、序列和公开索引。复核旧 G28 的生产断链保留。 |
| G35 | 仍缺（局部能力在） | caller `CompanyOperationsClockWiring::run_day_end`（`session/company_operations.rs:76–86`）→ `CompanyOperations::advance_civil_day` / `OperatingDayRun::advance`（`company/operations/day.rs:79–85`）→ 工商 `advance_day` 日常生产/销售/补货/管理费/ECL（`company/operations/industrial.rs:43–176`）。逐日阶段确有到期利息计提（`dispatch.rs:23–55`）并在次日排 `InterestAccrual`（`day.rs:198–201`）；没看见生产调用 `FixedAssetRegister::apply_depreciation`、`IndustrialBooks::accrue_income_tax`/`pay_income_tax` 或商业债务本金/利息支付。对应方法/底层账务能力有些存在，不构成 scheduler/operation 接线。报告日历/结账调用亦不自动补齐这些。不是要求股东分红/投资者补钱。 |
| G36 | 仍缺（冲击缺口另已有账） | 当前上市公司 assembly 将 `CompanyKind::Industrial` 硬编码（`session/company_assembly.rs:331–345`）；自定义 operations 的四类 flow delegate 存在不等于 SessionSetup 可装配四行业并贯穿日结与公开报告。`sample_company_shock` 无行业参数，均匀采样6种事件（`company/events.rs:210–234`）；caller 对所有公司遍历调用（`operations/day.rs:135–145`），无公司 kind 过滤；工业 flow 对 ProductionInterruption/AssetImpairmentSignal 消费（`operations/industrial.rs:58–77`），故银行/保险可能获得不适用工业冲击并公开。此遗漏属于已登记 G36 补充项，不新建编号。 |
| Q03 | 仍待定，部分冻结机制证实 | `simulation_policy_id` 当前版本守卫与持久化检查真实存在；CivilClockSave 完整冻结日历且digest重算。可据此反证“日历没有冻结”说法，但无法反证“规则 ID/会计、税务 profile 是否覆盖所有实际消费政策”；`RegulationProfile` 仅见文档语义，无同名生产结构。继续作为政策身份映射待澄清，不升级成新 G。 |

## 额外候选、反证与结论

- **商业债务支付**：`company-actions-design.md:25–37` 说商业借款属 K2/K3；底层 repay/pay interest 与日常 accrual 能力不是无代码，但生产 operations 日程目前 dispatch `InterestAccrual` 与合同 maturity。对照 G35，缺口是工商期末折旧、所得税及商债支付未进入经营闭环；它和 ADR-0024 不要求投资者资金回流完全不同。此项不重复开 G 编号。
- **混行业冲击**：`company-accounting.md:128–134` 记录行业节奏简化，但冲击采样未筛 CompanyKind 已由当前 owner/caller/consumer 路径复证；纳入已存在 G36 补充，不新编号。`CreditDeterioration` 属通用信用风险而非工业中断，不将所有行业冲击一概判错。
- **冻结政策**：文档所说“恢复存档优先于默认”的日历部分真实实现；`CivilClockSave.policy` 被序列化，`from_parts` 校验后重建。Q03 的真实缺口仍是更广 `RegulationProfile` 与固定 `simulation_policy_id` 的内容映射不明确，不能将日历快照结论泛化为所有公司会计/税务准则均被冻结，也不能说冻结整体缺失。
- **G15 可能反证评估**：若产品有意把 Official 条目解释成只标注命中日期而非整年权威覆盖，则现有行为没有类型错误；但文档明确规定公告可覆盖模拟结果，coverage entry 的年范围仍继续模拟节日与此承诺不合。需由产品/规则 owner 确认规范含义；本审计按已有总账所采用的“官方年度覆盖优先”契约保留 G15，不主张默认局已出错。
- 没有新建遗漏编号。法源资料取证日期是 2026-09-10，本轮无联网更新，因此法规当前有效性、policy fixture 完整性、报告期偏移行为均未声称通过。全篇为源码静态审查，无测试/构建验收。
