# Sweep 04：会计、公司行为与模拟日历逐章审计

- 审计生产基线：`b76ece3`；工作树 HEAD `4ad5a2e` 为审计合入提交，`git diff b76ece3 HEAD -- packages apps docs` 无输出，故以下为基线生产源码行号。
- 已连续阅读全文至 EOF：`docs/company-accounting.md` 251 行、`docs/company-actions-design.md` 94 行、`docs/simulation-calendar.md` 125 行，共 470 行。首轮会计输出的尾部截断已以 167–251 行补读；另外两篇再次完整读取确认 EOF。
- 已读根 `AGENTS.md`、`docs/principles.md`、ADR-0016 计划契约覆盖段 91–118 行、`docs/open-questions.md` Q12 122–140 行。以下不新增领域规则，法源状态沿用文档，未联网重新核验。
- 本轮只写审计记录；未运行测试、未改生产代码、未执行 Git 写操作。源码中的测试不能作为本轮运行结果。

## 逐章覆盖台账

| 原文章节与行号 | 原文承诺及当前路径 | 判定 |
|---|---|---|
| company-accounting 引言 3–12 | “未列入的业务即不支持”；机器清单 `tests/fixtures/company-model/policy-sources.json` 与 `tests/policy_manifest.rs` | 范围/取证登记，不能解释为全准则已实现 |
| §1 14–30 | “恢复存档优先用存档内政策数据，不用最新默认表覆盖”；新局 `session.rs:957`，恢复 `session/persistence.rs:260`、`session/persistence/v2.rs:237` 强制当前 `simulation_policy_id` | Q03 继续为契约澄清；没有找到允许跨政策恢复后覆盖的路径，不能因无同名 RegulationProfile 判缺失 |
| §2 总说明 32–35 | verified-official、游戏假设、blocked 分开 | 是覆盖状态，不是实现完成凭证 |
| §2.1 37–60 | 复式记账、五产物、会计分类、税务、合并、差错更正 | `accounting/ledger.rs`、`journal.rs`、`reports/`、`closing/mod.rs:168,189,239` 已有底座；单体日终 `session.rs:2184`→`books_mut:2223`→封月/年；合并生产披露仍 G28；工商期末折旧/税务仍 G35 |
| §2.2 62–70 | 工商销售、应收回款、坏账、采购生产 | `company/operations/industrial.rs:85` 生产、112 销售、136 补货、165 费用、174 坏账；`dispatch.rs:76` AR 回款；折旧/税务缺口不因这些已接线业务核销 |
| §2.3 72–85 | “客户存款吸收/支付”、应计利息“与结息”、贷款收回、ECL/费用 | `operations/bank.rs:55` 吸收、69 贷款、103 费用、106 ECL；`dispatch.rs:44` 计息、87 贷款收本息；存款支付处理器存在但编排未调用，见新候选 S04-N1；四行业会话装配仍 G36 |
| §2.4 87–111 | GMM、CSM、亏损合同组、保险六项列报；五项简化显式批准 | `operations/insurance.rs:54` 初始组、71 服务释放、75 赔案、76 赔付；`company/insurance/`、`accounting/reports/insurance.rs` 内核已有；G36 会话封账不通；D1–D5 不列新缺口 |
| §2.5 113–126 | 预售负债、交付收入、尾款、开发成本、借款资本化 | `operations/real_estate.rs:116` 开发、165 签预售、173 收款、190 排交付；`dispatch.rs:109` 交付及尾款；`real_estate/borrowing_costs.rs` 存在已登记假设；商业借款付款编排亦未找到，见 S04-N1 的地产补充 |
| §2.6 128–133 | 存贷款确定性日程、开发中断不延长时钟、完工不现售、保险/地产 CreditDeterioration 仅风险记录、赔案确定性 | `operations/bank.rs:54` 日程、`real_estate.rs:89,106,141`、`insurance.rs:73` 对应；这是简化许可。它只许可存贷吸收/发放日程，未许可取消到期存款本息支付 |
| §2.7 135–154 | D1 年末不落结转分录、D2 双口径重述、D3–D4 合并简化、D5 拒绝合并重述、D6 空类别/OCI、D7 比较宽松 | `closing/mod.rs:189` 年结生成版本、239 更正、376 scope 重述；`reports/equity.rs:77,79,89,91` OCI/分配零；这些已批准简化不得扩张成 G28 合并公开链无需实现 |
| §2.8 156–165 | 30 已完成交易分钟、2%/2x、权重 1+1+1+2，最大5 | `session/attention.rs:102,104,114,140,223` 与公共发现面 `information/acquisition.rs:266` 已有；本段参数不构成拟真承诺 |
| §3 167–173 | 结构化工具 UnsupportedContract；无外币 | `company/bank/mod.rs:54,76,86`、`company/insurance/mod.rs:59,82` 类型化拒绝；范围排除，无新增业务实现债 |
| §4 175–194 | 已公布生效安排、提前执行、fixture scope 不重叠 | 法源清单/规则冻结，结合 Q03；不要求模拟历史会计制度演进 |
| §5 196–211 | 年/中报期限、18:00 +0–7 日偏移、Q1 不早于年报 | `information/schedule.rs:74,89,110,125,135,145` 实现基准/偏移/窗口；`session/disclosures.rs:129`→157→182 公开；Q1/Q3 官方法源仍 blocked，不判代码缺失 |
| §6 213–232 | 再保险、分红/投连、PAA/VFA、银行非固定利率等显式拒绝 | 产品枚举支持面与 `UnsupportedContract` 见上，已排除范围；不能要求新增这些业务 |
| §7 234–251 | CAS1/4/6/8/18、28/31/32、税法/2026休市/上市规则取证受阻 | 法源债单列；已实现算法不能冒充官方合规，blocked 也不能免除已承诺且有底座的经营编排 |
| company-actions 引言、§1 1–18 | “设计文档，无运行时代码”；商业债务“照常实现” | 股东行为无实现义务；商业债务独立检验，G35 与 S04-N1 不受股东排除保护 |
| §2 总题、2.1 20–37 | 商业借款还款利息税费属于已实现范围；可转债未来 | `industrial/repayment.rs:18,61`、`real_estate/debt_service.rs:14,57` 处理器已有，经营 caller 缺失；可转债不列缺口 |
| §2.2 39–52 | 利润留存、不自动分配、权益表分配恒0、初始股本事实 | `reports/equity.rs:79,91` 结构性0；`session/company_assembly.rs:343` issued_shares 从 total_shares 装配；无分红实现债 |
| §2.3 54–65 | 固定总股本；交易转手不改变已发行股本；登记子账未来 | `CompanySpec` 与装配固定股本一致，未来登记队列/接口不列当前缺口 |
| §2.4 67–76 | 股东税未来；公司增值税/当期递延所得税本范围 | 工商 `expenses.rs:127,205` 已有计提/支付内核、`company_assembly.rs:424` 配政策；缺 caller 为 G35；税率官方取证债另列 |
| §3 78–86 | 本发布无除权除息，未来先登记规则及新格式 | 显式未来，无当前实现债 |
| §4 88–94 | 无分红/发行/回购/清算/补钱/队列/API | 对公司/会话源码检索未发现对应运行时实现；ADR-0016:106、Q12 决定支持排除 |
| simulation-calendar 引言、§1 1–23 | 真实 Gregorian、闰年/合法日期、交易时段不伪造分钟 | `calendar/date.rs:91,160,183`、`session/civil_clock.rs:188` 已有日期/双时钟；没有以休市推进生成行情 |
| §2 25–32 | 2030 默认、2000–2099开局、1998初始化、越界不改状态 | `civil_clock.rs:214,221` 默认/验证、409 上界拒绝；`session.rs:2088` checkpoint→2093 回滚；`calendar/holidays.rs:40,50` 运行/查询门 |
| §3 总题、3.1 34–48 | 2026原文未核验；未来模拟；取得官方原文再升级 | `CalendarPolicy::default_v1` (`policy/mod.rs:57`) 无官方覆盖；法源债，不要求捏造官方通知 |
| §3.2 50–55 | 历史/初始化年份缺原文标 SimulatedHistorical | `policy/mod.rs:149` year_label 独立区分历史/未来/通知未核验；无新增遗漏 |
| §3.3 57–68 | “对没有官方覆盖的年份”用模拟假日；公告可覆盖结果 | `calendar/holidays.rs:72` 官方覆盖未命中休市日仍走模拟假日88，G15 保持 |
| §3.4 70–79 | 标签入存档/诊断；恢复用冻结政策 | `policy/mod.rs:149` 标签、`civil_clock.rs:238,243` 保存政策恢复；已实现，Q03 不由日历冻结缺失构成 |
| §4 81–89 | 当日经营/18:00再推进；政策保存“沪深分别”的覆盖 | `session.rs:2100` 时钟、2105经营、2110结账、2114披露；冻政策见上；沪深不同覆盖进入单会话的问题记录 S04-N2 候选 |
| §5 91–105 | RegulationProfile 固定、早期统一现行规则、blocked 不补默认 | Q03：政策ID硬门存在；无年份自动改规则路径，仍需文档说明ID与规则集合映射 |
| §6 107–114 | ACT/365F自然日、半偶舍入、合同累计余数、非交易日结算不偷股票顺延 | `company/contracts.rs:26` basis，`industrial/loans.rs:230,248,260` 半偶和余数，`operations/dispatch.rs:16` 自然日到期；S04-N1 缺支付不能由正确计息数学抵消 |
| §7 116–125 | 自然年排期、窗口、周末可公布 | schedule 与 disclosure 生产链见会计§5；已实现。无季度法源不能写法定验收通过 |

## 既有总账复核

- **G15 保持**：官方年度条目存在、当日不在官方休市区间时，`holidays.rs:82` 只在 covers 时返回，88 仍按模拟假日关市。文档59明确只对“没有官方覆盖的年份”回退。默认无覆盖，不声称默认2030局已触发。
- **G28 保持**：会计49及 ADR-0016:97–101 的合并完整交付并未被 D1–D6 取消；`session/disclosures.rs:186` 与 `information/prehistory.rs:163` 仍只 Standalone，装配 `company_assembly.rs:344` group_parent None。往来抵销 `consolidation/eliminate.rs:85` 仅成员/科目检查，122仅镜像等额、149/155直接使用申报额，无成员真实账面上界检查；不是声称默认无集团游戏已超额抵销。
- **G35 保持并建议补范围**：工商固定资产/债务在开局 `company_assembly.rs:383–405` 已存在；`operations/industrial.rs:43` 完整处理体没有 `depreciate_month/accrue_income_tax/pay_income_tax/pay_interest/repay_principal`，`session.rs:2184` 封账也没补；利息计提已有 `dispatch.rs:41`，不能写成利息全部不存在。新 S04-N1 是银行及地产经营负债支付的同类未接线，当前G35标题只覆盖工商会遗漏。
- **G36 保持**：装配 `company_assembly.rs:341,430` 统一 Industrial；非工商 `operations/config.rs:43` unreachable，月末 `session.rs:2223`调用。四行业纯内核不等于四行业会话入口/封账/查询已打通；默认工商不声称触发 panic。已获批 CreditDeterioration 只记录，不追加为缺口。
- **Q03 保持为澄清**：当前ID限制（setup957、恢复persistence260及v2:237）为实际反证；缺同名结构不足以定缺失。日历完整政策恢复另外已有 `civil_clock.rs:243`。

## 新候选与反证

### S04-N1：银行定期存款本息付款未进经营到期编排；地产经营借款付款亦未接线

原文 `company-accounting.md:76` 明确“客户存款吸收/支付”，78 明确实际利率应计“与结息”；`company-actions-design.md:15,24` 将商业债务还款/利息列入当前实现。`company-accounting.md:130` 的确定性存贷吸收/发放日程简化不包含免付到期债务。

当前银行 `operations/bank.rs:55–67` accept_deposit 创建明确 maturity 的 DEP 合同，随后无 scheduler.submit；同文件92–99仅贷款 LN 入到期队列。`operations/dispatch.rs:75–120` 完整 match 仅 AR、LN、DL，无 DEP 支付。已实现 `bank/deposits.rs:250` withdraw_deposit、`bank/interest.rs:173` pay_deposit_interest；检索整个 `company/operations/`、`session.rs`、`session/`，这些以及工商/地产 pay_interest、repay_principal 均无 caller。滚动利息 `operations/injections.rs:100–123` 只注册 InterestAccrual，不能负责付款。

结果是配置 Bank 的 CompanyOperations 自然日经营可以吸收及计提负债，但不按合同到期付存款本息；修复G36会话接线后仍缺该业务。银行 loans 支付回收已有 `dispatch.rs:87–105`，应保留作为反证，不能说全部到期处理缺失。地产 `real_estate/debt_service.rs:14,57` 也有付款处理器而无经营 caller；地产日常 flow 不主动新建贷款，故这一部分仅在存在显式开局/注入经营借款的输入下成立，不能声称默认工商局出现地产违约。建议扩充G35，或单列跨行业经营债务支付项。

建议短边界验证：Bank 自定义 CompanyOperations、少量 TermDeposit、自始日至到期、断言principal/accrued_payable付款及现金变动；不足现金应产生 PaymentFailed/Overdue 且不能凭空支付；周末到期使用自然日而非股票顺延。地产带借款 fixture 独立验证到期本金/利息。未运行这些测试。

### S04-N2：沪深不同官方覆盖进入混合会话后仅按首只股票所运行（需父审计再判断承诺范围）

文档 `simulation-calendar.md:88` 要求“沪深分别的已知休市覆盖”； `CalendarPolicySpec` (`calendar/policy/mod.rs:37`) 支持按 exchange/year 官方条目，`validation.rs:47`不要求两所条目相同。 `session.rs:1389` 新局、2801–2804恢复都选 `setup.stocks[0].exchange`，`civil_clock.rs:204–210` 明确会话只取首只所，整个会话 phase323只查询该所。

反证：默认官方覆盖为空，两所模拟同轨，当前正常默认局不触发；文档要求分别保存覆盖不一定独立承诺任意异步两所交易会话。故先列候选，不能把真实两所不同休市当成已发生事实；若当前只支持共用日历，应在恢复/配置边界显式拒绝不同所覆盖而不静默按首所。它与G15不同：G15为年度模拟回退优先级，S04-N2为不同交易所消费同一会话时钟。

## 不计入当前代码缺口

公司分红/融资/回购/注销/清算分配、红利税、除权除息、可转债、股东登记子账、再保险/投连/分红险/PAA/VFA、银行FVTPL/FVOCI/套期、外币、历史逐年会计政策演进、CAS25及CAS33已登记简化。法源取证阻塞继续单列（CAS1/4/6/8/18/28/31/32、税法、2026休市、季报上市规则），不以 blocked 本身添加代码缺口，也不写已解除。
