# S03：两份公司计划与 ADR-0016 全文实现复核

## 范围与阅读记录

2026-10-03 在独立 worktree 只读追踪产品代码；新增本记录，不修改产品、不执行 Git 写操作、不运行长测试。指定产品基线为 `b76ece3`；实际 worktree HEAD 为 `4ad5a2e`。`git diff --stat b76ece3 HEAD` 只有十份审计文档差异，相关产品源码没有差异，因此下述产品证据适用于 `b76ece3`。没有用源码存在或任务 `[x]` 宣称运行验收通过。

已连续全文阅读：根 `AGENTS.md` 123 行、`docs/principles.md` 92 行；`docs/superpowers/plans/2026-09-10-company-information-npc-intentions.md` 700 行，分段 1–180、181–350、351–530、531–700；`.omo/plans/company-information-npc-intentions.md` 713 行，分段 1–193、194–363、364–543、544–713（末段输出截断后补读 664–713）；`docs/decisions/0016-fundamental-factor-model.md` 119 行。全文读取 ADR-0023/24/25/26，另查现行 `company-accounting.md`、总账与 `reaudit-engine.md`、候选核销记录。原文引用以下以 700 行的正式计划为准；同内容 `.omo` 版通常偏移 +13 行。

## 旧决定的适用关系

- ADR-0016:97–105 明确候选经营范围及“小而完整”顺序被 K2/K3 完整交付与六波计划取代；不能以 ADR-0016:25 原候选措辞取消四行业、合并、五产物。
- ADR-0023:15–27 核销真实前史导入与 C06 授权数据校准；保留虚拟前史和真实撮合的验证，不取消会计/税务制度法源。
- ADR-0024:20–31 核销投资者资金循环；公司商业收付/利息/税费仍需实现。现金不足风险披露不等于给投资者补钱。
- ADR-0025:12–33 取代任务 27/30–32 的公共日内保存与日内委托存档要求；内部内存快照及失败回滚仍存在，跨日计划/经历/随机流仍需保存。
- ADR-0026:41–48 已接机构真实失败衰减及信心反馈，且明确仅改信心、不改估值/预测；不能重新要求机构经历修改预测，也不能用机构实现核销 G08 散户日期衰减。
- 两计划逐行 diff 显示 `.omo` 的目标仓位旧公式、10%/30%共同留现金和数量配额没有同步 ADR-0019/21。现行以正式计划:155/164/180 的修订为准；不把撤销配额、留底当遗漏。自由并发整局逐字一致按 `.omo`:5 与现行 testing 契约核销。

## K 契约映射

| 契约与原文 | caller → owner → consumer | 判定 |
|---|---|---|
| K1:69–80 日期/双时钟/日历/规则 | `GameSession::new` / `end_civil_day` → `calendar/policy/mod.rs:57` / `session/civil_clock.rs` → civil events、setup/save、三宿主 | 主干存在；G15 官方覆盖反转边界、Q03 规则 ID 关系继续登记。 |
| K2:82–93 公司/金额/原子账/前史/资金 | `session/company_assembly.rs:58` → `CompanyRegistry` / `CompanyOperations` / `Books` → 日终与严格存档、报告 | 主干存在；现金不足风险材料新漏接 S03-N1。 |
| K3:95–110 四行业/税务/合并/结账 | `session.rs:2105` → 四行业 flow / `close_accounting_periods` → `disclosures.rs:157` / 公共查询 | G28 合并、G35 工商期末和债务支付、G36 自定义非工商会话仍有缺口；法源债单列。 |
| K4:112–123 经营冲击/披露/本人获取 | `CompanyOperations::advance_civil_day` → `OperatingDayRun` / `DisclosureDispatch` → `PublicLibrary` / `NpcInformationState` | 大部分已接；G36 行业不适用冲击公告仍在；S03-N1 付款失败报告丢弃；S03-Q1 延付需厘清。 |
| K5:125–136 身份/记忆/经历 | accepted attention → `InstitutionDecisionRoot` / `RetailExperienceState` → 候选分析、计划/执行 | G07 散户分析/G08 日期衰减、Q02 历史读取已登记；另有 S03-N2 价格记忆修剪漏接。 |
| K5a:138–158 数学/本人已知报告/迟滞 | root 获知 → `BeliefBook` / `fundamental/valuation.rs:159` / blend → `decision_chain/lifecycle.rs:430` | G06 逐年折现、G09 中期材料、Q11 更正/违约 cause 延续；机构经历按 ADR-0026 核销旧预测要求。 |
| K6:160–170 计划/预算/报价/成交 | root 候选 → lifecycle / allocation / parent order → 真实 submit/cancel/receipts | 主干存在；G38 新机会分类未接；另有 S03-N3 淡出候选范围漏接。 |
| K7:172–180 存档/公共查询/DEV | 日结/host query → ProtocolSession / public DTO / company coordinator → UI、三宿主、持久保存 | 日終保存/公开查询/隔离基础存在；G37 真实订单关联、G01–G05 宿主局部边界、完整三宿主/发布/视觉验收债保留。 |

## 42 项任务逐项映射

此表逐项覆盖实施内容、Acceptance 与 happy/failure QA 的任务族；“存在”表示生产机制或相应验证入口已定位，不表示本轮已运行，原测试更名/拆目录不作为缺功能。涉及验收工具由 S08 接续独立深核。

| 任务、正式计划起始行 | 当前入口/所有者/输出 | 本轮结论 |
|---|---|---|
| 1:267 before | `scripts/simulation/baseline-run.mjs` → fixture/release示例 → manifest | 工具存在，历史 before 不回写；未重跑。 |
| 2:276 法源/范围 | `docs/company-accounting.md`、policy fixture →政策校验→四行业业务 | 已有矩阵；CAS8 等原文取证债保留，不当整个内核缺失。 |
| 3:286 抽取 | `session/pipeline/`、`session/decision_chain/` → root/route/settlement → commit | 接缝存在；旧逐字 replay 被现行受理契约修订。 |
| 4:295 日历 | setup → `CalendarPolicy` → clock/save | 主干存在，G15/Q03。 |
| 5:304 civil | `end_civil_day` → civil clock/经营/披露 → `CivilUpdate` | 主干存在，休市不造 tick；本轮未跑边界。 |
| 6:313 总账 | business handlers → Journal/Books/Ledger →报告 | 原子批次及 checked i128 内核存在；不据底层个别失败面要求全方法强事务。 |
| 7:322 开局 | `company_assembly.rs:58` → registry/operations/prehistory → Session | 默认工商及四行业纯内核存在，自定义会话 G36；S03-N1。 |
| 8:333 工商 | `operations/industrial.rs:43` → IndustrialBooks → journal/reports | G35；现金不足捕获已做，但风险公开链 S03-N1。 |
| 9:342 银行 | `operations/bank.rs` → BankBooks →行业报表 | 内核存在，会话闭环 G36。 |
| 10:351 保险 | `operations/insurance.rs` → InsuranceBooks →行业报表 | 内核存在，会话 G36；保障日赔案为显式游戏假设。 |
| 11:360 地产 | `operations/real_estate.rs` → RealEstateBooks →报表 | 内核存在，G36；确定性进度、信用冲击仅记录等已登记简化不重开。 |
| 12:369 合并 | `consolidation/mod.rs:85` →抵销底稿→合并 ReportSet | G28，含 caller真实账面上界；不要求所有默认公司有子公司。 |
| 13:378 五产物 | `reports/mod.rs:187` / `closing/mod.rs:168` → registry → public DTO | 五产物/不可变版本存在；G28/G36生产范围；已登记 D1–D7 不按旧计划重造。 |
| 14:387 经营 | daily loop → due/四行业 flow → `CompanyDayReport` | G35/G36，新增 S03-N1；S03-Q1 非确定事实待确认。 |
| 15:398 披露 | `disclosures.rs:99` → PublicLibrary →公共索引 | 定期/冲击公告已接；G28 单体范围；付款失败不进入公告 S03-N1。 |
| 16:407 获知 | `roots.rs:349` → `record_acquisition` → NpcObservationContext | 本人获取入口存在；G09中期只获取不进入估值更新。 |
| 17:416 profiles | StrategyFactory → AnalysisProfile/BeliefBook →根分析 | 工厂内核存在，生产 Retail仍G07；不以profile测试核销。 |
| 18:425 估值 | root → fundamental facts/forecast/valuation →个人区间 | 三路径存在，G06/G09/Q11。 |
| 19:434 记忆/技术 | root → TechnicalObservation / PersonalPriceMemory →blend | Q02主动读取留痕，S03-N2修剪；已有指标不等于个人记忆上限兑现。 |
| 20:443 经历 | settlement/观察 → retail/机构experience →信心/风险 | 机构 ADR0026已接，Retail G08；不重复信心扣减。 |
| 21:452 状态机 | lifecycle → TradingPlan/PlanBook → parent execution | Active/Paused/Completed/Terminated 与真实 fill 入口存在。 |
| 22:463 预算 | account execution → `allocation.rs:39` → child grants | G38优先输入；S03-N3候选集合越界；不恢复共同现金留底。 |
| 23:472 紧迫度 | `decision_chain/urgency.rs` →冻结个体policy→ quote | 已接急跌/个体风险暂停、恢复观察、报价保护。 |
| 24:481 真实执行 | plan chain → parent/lifecycle →路由与receipts | 已接真实撤报/成交/日终清理；日内save需求被ADR0025替代。 |
| 25:490 关注 | accepted attention → `sample_discovery_stock`/watchlist →root candidate | 权重与watchlist修剪存在；Retail曝光仍G07相关边界；S03-N3淡出未过滤信念候选。 |
| 26:499 全链/V删除 | new/step → company assembly/root/pipeline →撮合价格 | 共同V已删；G07/G08/G09/G36等局部生产断链不能被Task26勾选核销。 |
| 27:508 存档 | 日结候选 → persistence →严格restore | 新格式/原子替换存在；ADR0025取代日内委托序列化。 |
| 28:517 场景 | company_scenarios测试 →真实GameSession →对账 | 测试入口存在；不能据测试源码宣称全情景完成，未运行。 |
| 29:528 公共DTO | `company/query.rs:21` →`information/queries.rs` →Rust生成TS | 金额/ID无损及查询基础存在，Q03身份待澄清。 |
| 30:537 WASM | `lib.rs:406` →session report query→Worker/Coordinator | 已有公共查询桥，G01–G05的宿主边界由S04复核。 |
| 31:546 Server | `routes.rs:499` →actor→public report | 查询/鉴权/真实body边界基础存在，宿主缺口另册。 |
| 32:555 Tauri | `lib.rs:159` →actor→report query | IPC查询存在，G03–G05等另册。 |
| 33:564 公共更新 | host stream →company-query-coordinator/company-slice→UI | 空态/错误/旧generation防污染基础存在；未运行E2E。 |
| 34:573 界面 | CompanyPanel/StartDateInput →统一host→四表/附注 | 展示基础存在；非工商/G28上游缺口不能靠UI核销；完整视觉证据债保留。 |
| 35:583 DEV | `record_plan_root_diagnostics:754` →trace collector→NpcDecisionInspector | G37空events；能力隔离基础已有，未跑真实制品探针。 |
| 36:595 因果统计 | pipeline/root→diagnostics/causal→离线baseline | 指标内核存在，G37关联不足；不以源码存在证明指标全验收。 |
| 37:606 三宿主 | 真WASM/HTTP/IPC测试入口及现行验收工具→runtime→证据 | 原同seed自由调度整局一致已取代；未取代的实际边界/故障/频率矩阵仍缺完整证据。 |
| 38:615 敏感性 | baseline-run→after/sensitivity→manifest | ADR0023核销C06真实数据；四行业400自然日、多seed等有效验收债保留。 |
| 39:624 规模 | session/company_scale tests →restore/step→成本输出 | 入口存在，现行事实对账替代自由调度字节等价；完整规模证据由S08核对。 |
| 40:633 release | 实际发布制品/manifest/runtime probe→三宿主→记录 | 旧脚本名缺失不独立算漏实现；真实制品隔离能力与尚缺证据分别登记。 |
| 41:642 文档 | current ADR/规则/工作状态 →实现/证据引用 | `.omo`现金保留/数量限制文档漂移已识别；不能重开撤销要求。 |
| 42:651 全门禁 | 当前验收runner→测试/源identity→manifest | G39现行受理事实重放边界；不将历史验收或短测扩张为本基线全绿。 |

## 新确认候选（送主控独立复核后再编号）

### S03-N1：现金不足已捕获，但业务风险状态/公开材料丢失

原文：K2:92 明确“资金不足生成 PaymentFailed/Overdue 业务状态和公开风险材料”，任务 8:339/14:393 要正确逾期/支付失败业务事件。ADR-0024:28 不取消公司商业收付，也未批准取消经营风险材料。

caller → owner → consumer：`session.rs:2105` → `CompanyOperationsClockWiring::run_day_end`（`session/company_operations.rs:84`）→ `operations/industrial.rs:99/156/167` 把现金不足放进 `CompanyDayReport.payment_failures`（`operations/core.rs:64–80`）；日终 caller 对整个返回值直接 `?;`，不保留记录。`OperatingCompany` 权威字段（`operations/core.rs:21–27`）没有失败/逾期集合；`disclosures.rs:112–124` 只读取当前 active shock，`AnnouncedEvent`（`information/publication.rs:126–131`）只支持 `ShockKind`。因此公开风险材料入口没有消费已发生支付失败，日终save也不保留该日报告。

影响：合法现金不足仍能使经营日继续、不透支，这是已有正确行为；缺的是用户/NPC可获知并在恢复后保留的真实失败事实。不能称整个 PaymentFailed 内核没实现，不能用更大的虚构授信或给证券账户补钱处理。该漏接独立于 G35“还未调度折旧/税费/商业债务支付”；即使每日管理费用/采购等已实现支付失败也受影响。最小测试应在真实 Session 日结、恢复、公共材料查询中断言失败事实保留且不捏造现金/成交。

### S03-N2：PersonalPriceMemory 的 held/active+8 修剪没有生产 caller

原文：K5:135、任务19/25:435/491 明确有限个人记忆和淡出；这是行为状态边界，ADR0019撤销的是世界/请求任意容量配额，不是个人注意力/记忆模型。

caller → owner → consumer：`InstitutionDecisionRoot::observe_personal` 在 `roots.rs:323–337` 对 candidates 逐股 `price_memory.observe_price` → `PersonalPriceMemory.stocks`（`experience/price_memory.rs:140–160`）保存条目 → `PlanPersonalState::install`（`decision_chain/personal_state.rs:105–112`）与日终save保留。相同 root 末尾 `roots.rs:481–486` 算 protected 并仅调用 `watchlist.prune`；真正 `PersonalPriceMemory::prune`（`price_memory.rs:182–197`）只有测试调用。

影响：自定义超过8只股票的合法世界里，已平仓/无活跃计划的观察条目继续保留，未兑现上限。默认5股无法展露此边界。`persistence.rs:1184–1193` 以全世界股数+8为粗界限，不能据此声称save必失败；该守卫不证明真实未持仓+8模型正确。与 Q02“主动读取历史留痕”是两种断链，可共用修复批次但不能互相核销。最小测试应跨多股实际观察/退出，并在保护持仓与活跃plan后验证最多8个其余价格记忆，恢复后一致。

### S03-N3：淡出关注后，旧 belief 股票仍无限加入生产分析/信息/新计划候选

原文：K6:165 限于本人持仓/关注/本轮发现；任务25:491–496 要退出、计划终止后可以淡出。保留已知报告历史不等于自动每次继续观察该发行人。

caller → owner → consumer：`roots.rs:322` → `root_candidate_codes`（`decision_chain.rs:299–310`）无条件 `candidates.extend(belief.entry_stocks())` → `roots.rs:323` 写新观察价、349获取该发行人的新公开材料、468再次分析、476推入 lifecycle；`decision_chain/lifecycle.rs:517–565` 在没有活动计划且有合法方向时允许重新建立新计划。`watchlist.prune` 从集合驱逐股票没有给上述候选入口提供过滤条件，旧 belief entry 也没有因淡出被删除。

影响：曾产生过belief、现在无持仓/活动plan、已被watchlist淘汰且本轮未重新发现的股票，仍每次获得新观察/材料并能重新建计划；个人关注淡出无法完整生效。这不要求删除本人既往获知/预测事实，也不要求所有watchlist股票每次同步分析；最小修复是对“本次候选资格”施加真实关注约束，保留历史状态。与 G38预算优先分类、S03-N2价格记忆修剪各有独立 consumer。最小测试固定淡出股，保持本人历史belief/报告，验证本人未重新发现前无新观察/获知/新计划，重新发现后恢复资格。

## 仍属待澄清而非新确认 G

S03-Q1：K4:116 的“客户延付/信用恶化”目前合并为 `ShockKind::CreditDeterioration`（`company/events.rs:132`）；`operations/state.rs:63` 仅增加 credit risk，工商 `industrial.rs:174` 将其用于坏账准备；到期 `dispatch.rs:76–84` 无条件对全部应收调用 collect，不延期回款。正式会计文档:130–133 仅批准保险/地产信用冲击只记录，未见工商延付范围登记。不过斜线措辞可能允许信用恶化替代独立延付机制，因此本轮只提需要澄清的契约，不能冒充已确认漏实现，更不能把坏账准备称作完全没有实现。

## F1–F4 与 DoD 映射

F1（正式计划:664 / `.omo`:677）：42任务/全diff/四行业/三宿主证据；本记录补需求到生产链，G28/G36与新S03-N1–3阻止全面完成声明。F2（668/681）：会计/交易/日期/单位/合法订单核对由独立主控review承接；不以审计新增实际交易制度。F3（672/685）：实际三宿主旅程、375/768/1280视觉/键盘/空态/错误态；本轮未运行，保留真实验收债。F4（676/689）：范围与source identity/原始输出/旧产物检查；本记录只修改工作文件，旧真实数据/派钱/兼容/公共日内保存已按最新决定核销。

DoD 1（692/705）：四行业五产物有内核，G28/G36闭环不足；2（693/706）：日期主干/G15/Q03；3（694/707）：G06/G07/G09/Q02与候选N2/N3；4（695/708）：计划真实生命周期已有、G38/N3；5（696/709）：不派钱边界已有、G35/N1保留；6（697/710）：严格日终save按ADR0025兑现主干，N1/N2状态边界待补；7（698/711）：公共查询/DEV基础已有、G37和宿主验收保留；8（699/712）：多seed/规模/长期证据不由本次静态检查核销；9（700/713）：不能把42勾选和历史审查当本基线全面完成。

结论：现有总账 G06–G09/G28/G35–G38 与 Q02/Q03/Q11 的大部分判断仍成立；新增三条可静态证明的生产闭环候选 N1–N3，另有一条契约候选 Q1。均未运行行为复现，最终升级 G 需主控对 caller→owner→consumer 和最新决定独立复核。
