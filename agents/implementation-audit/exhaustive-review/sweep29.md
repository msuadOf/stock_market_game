# sweep29：Task 29／3／30 历史复核与当前 caller 对照

## 全文与基线

- 主控指定产品基线 `b76ece3`，在 `.worktree/implementation-reaudit` 检查 merge HEAD 同产品代码。
- 已遵守此前读取的根 `AGENTS.md` 与 `docs/principles.md`；本批只新增报告，不写产品、测试或 Git。
- `.omo/evidence/company-information-npc-intentions/task-29-review.md` 全文 256 行，连续读取 1–256。
- 同目录 `task-3-review.md` 全文 21 行，连续读取 1–21。
- 同目录 `task-30-review.md` 全文 58 行，连续读取 1–58。
- 合计 335 行。历史日志中的命令通过／阻塞不是本轮运行结果；本轮只读追踪，没有运行测试。

## Task 29 逐章状态

| 章节／原文行 | 当前状态 | 生产证据与反证 |
|---|---|---|
| Final Re-Review／P1 #1 Closed，7–17 | 已解决，不重开旧 V 缺口 | 当前全文搜索 `apps/web/src` 未找到 VError／VParams／v_params／v_initial。当前统一协议只接受现行事件，`apps/web/src/host/protocol/parse.ts:32` 有显式 allowed tag，旧 V event无入口。 |
| Strict Company Operations Shape，19–31 | 已解决，旧 REJECT 被后续修正覆盖 | `apps/web/src/save/save-schema.ts:3` 只调parseStrictSaveEnvelope；`schema/company/operations.ts:50` exact根并逐公司解析。`schema/company/books/industrial.ts:23` inventory先record、exact items再递归项；`schema/company/accounting/chart.ts:17` 校验name／element／boolean；`schema/company/policies/flow.ts:48` 四行业有独立严格parser。历史inventory=7／name=7／金额={}均不再能通过这些形状守卫。跨引用、账务、所有权和时间深度约束继续归Rust。 |
| Public Query, Defaults, Scope，33–36 | 已实现公开投影、精度与分页；五股默认仍在 | `packages/engine/src/company/query.rs:462` 只从PublishedReport构造DTO；`:491` ID字符串；`:465` 金额用to_yuan_string。`information/queries.rs:31` 默认20且0/>100显式拒绝，`:50` cursor必须同公司且已公开，`:66` 按不可变ID排他取下一页。`apps/web/src/config/defaults.ts:37` 默认五股；未把集团G28或股东行为误核销。 |
| Final Verification／Git-Isolation／Residual Risks／Disposition，38–68 | 历史验证及过程证据 | 不重跑旧types生成、隔离Git、199测试、mature-save或LSP；不把历史APPROVE当当前全量验收。原fixture portability问题已有后续仓内fixture，不凭历史绝对路径登记新生产缺口。 |
| Reopened Civil and Disclosure／Missing Event，70–80 | 已实现 | `packages/engine/src/session.rs:285` CivilDateAdvanced；`:294` CompanyDisclosurePublished；`:355` seq分派；`apps/web/src/host/protocol/parse.ts:99`／`:102` 明确解析各字段，生成event类型持续使用现行Rust来源。 |
| Authority, Ordering, Privacy, Exactness，82–87 | 已实现公开插入后再发事件及日界 | `session/disclosures.rs:117` publish_announcement成功后才收ID；`:144` publish_scheduled后收ID。`session.rs:2105`经营、`:2110`封账、`:2114`披露；`:2143`／`:2164`发公开ID事件，`:2172`最后发CivilDateAdvanced。事件不含books／journal／NPC私有状态。休市日经营／披露仍执行，不把休市推进当价格tick。 |
| Transaction, Sequence, Restore Safety，89–93 | 当前事务机制已演进，行为守卫仍在 | 历史“回滚完整SaveSlot”已改内存shadow/checkpoint（`session.rs:2088`／`:2094`）；外层`session/protocol/civil/session.rs:283`校验完整CivilUpdate失败回滚、成功后才安装候选。内部回滚不是公共日内保存；恢复公共档仍须完整日结。旧文字不能要求退回序列化回滚。 |
| Reopened Verification／Web Limits／Git Receipt，95–122 | 历史阻塞／旧宿主丢事件已被后续接线覆盖 | `apps/server/src/actor.rs:1176`准备CivilUpdate、`:1191`广播完整更新；`apps/desktop/src-tauri/src/actor.rs:913`准备、`:919`emit；`apps/web-wasm/src/lib.rs:394`返回EngineUpdate::CivilUpdate；`apps/web/src/host/wasm-tick-loop.ts:66`发布统一协议。Server不再只是调用后丢弃CivilDayEndReport.events。旧并发缺文件及format/LSP阻塞不直接作为当前缺口。 |
| Re-Review／Old P1 #1／P1 #2／Integrated Parser，124–169 | 旧未通过公司形状已经修正 | 以后续最终段11、19–31和当前严格parser为准。不能因为历史155行复现成功就声称当前仍接受inventory scalar。精度、K7事实恢复遵循新格式，不要求旧fixture无条件迁移。 |
| Re-Review Gates／Generated Baseline／Disposition，171–194 | 历史门禁 | 不复述188通过为当前通过，不操作Git写树，不根据当时REJECT重复登记已修问题。 |
| Scope Audited／Original Findings Status／Confirmed Results，196–213 | 当前代码优先于残留状态文字 | 203行仍称残留P1，与顶部最终闭环冲突，按11行“superseded”处理。210行所有host查询capability=false是历史当批边界；当前worker／remote／tauri均true且有生产桥，不登记“宿主不能查报告”。 |
| Isolated Git Baseline／Gate Results／LSP／Required Disposition，215–256 | 历史过程，最终处理已覆盖 | 旧合成tree、hash、187测试、clippy/fmt/LSP结果均不作为本轮事实；256行旧停止条款已被顶部闭环覆盖。 |

## Task 3 逐章状态

| 章节／原文行 | 当前状态 | 生产证据与边界 |
|---|---|---|
| 1–5：独立review身份、旧commit、日期 | 历史纯移动review | 它不是当前新增产品需求；不要求今天源码与2026-09-10逐行恒等。 |
| Method，7–8 | 历史行为保持取证 | 原父commit／移动commit的line membership及有序切片是原批次证明；本轮不制造旧输出或运行Git写操作。 |
| Findings by gate，10–15 | serde／导出／RNG顺序原则仍有效；锚已获后续更新 | `packages/engine/tests/extraction_replay.rs:3` 说明现行受控single-worker锚；`:35`起保留历史锚演进记录；`:242`同seed事件与中末档，`:266`固定digest，`:281`seed扰动，`:296`事件顺序扰动。不把旧纯移动证明当后续所有功能无需回归，也不要求恢复旧V或多worker自由调度字节一致。 |
| Tests／Verdict，17–21 | 历史通过，未重跑 | 旧467 passed不能称现行全量通过；没有发现这份纯移动review独有未落实产品要求。 |

## Task 30 逐章状态

| 章节／原文行 | 当前状态 | 生产证据与反证 |
|---|---|---|
| Initial scope／Findings，5–9 | report-by-ID桥与公共DTO严格校验已接 | `apps/web/src/host/worker-host.ts:369` publicReportById发送请求并normalize；`wasm-worker.ts:314`实际调用；`apps/web-wasm/src/lib.rs:418`只委托公共查询。`public-report-normalize.ts:11` exact递归形状；`:53`ID字符串；`:57`金额字符串；`:154`／`:161`入站公开报告／页，消除旧residual弱校验。 |
| Confirmed，11–18 | 公开／精度／候选恢复／请求匹配已有 | `company/query.rs:462`公共投影；`wasm-restore-transaction.ts:18`先restore candidate，`:19` snapshot后才`:21`replace、`:23`drop旧handle；`worker-request.ts:54`／`:55`匹配requestId及旧请求generation。生成wasm-pkg来源属于构建证据，未伪造当前官方重建通过。 |
| Follow-up，20–30：numeric maps | 已实现 | `apps/web/src/host/serde-normalize.ts:64` prepareSaveForWasm转换全部列明account maps，`:94`plans.plans；当前Worker改用restore_json标准JSON入口（`wasm-session-slot.ts:58`），不需要强制调用旧numeric Map兼容helper。 |
| Follow-up，26–27：nextGeneration推进拒绝 | **新候选 C29-01：异常恢复响应的单调性守卫缺失** | 见下节，当前caller只校验正安全整数，真实binding仍推进，但历史已承诺的非推进响应拒绝未保留。 |
| Public report period-date repair，32–44 | 旧跨宿主YYYY-MM缺口已修 | 后续46–58中央修正优先；`company/query.rs:493`共用period_end_date并to_iso；`information/publication.rs:181`按末月下一月首日prev，支持闰年／12月；WASM`:406`／`:418`无本地算期末。 |
| Central cross-host re-review，46–58 | 当前统一period合同符合；Desktop环境限制作历史证据债 | Server `actor.rs:1436`／`:1439`、Desktop`:1001`／`:1010`直接输出相同engine DTO；各Webhost共同normalize并`:61`验证真实日期。源接线不等于GTK/WebKit原生矩阵跑通。 |

## 新候选 C29-01：Worker restore nextGeneration 必须推进的守卫遗漏

原文：`task-30-review.md:26–27`，“The restore response now carries post-restore nextGeneration, which the host validates and adopts; the focused test rejects a non-advancing generation.” 这是旧review明确承诺过的异常输入拒绝能力，不是从同名字段推导新需求。

当前路径：

1. `apps/web/src/host/worker-host.ts:79` restoreWorkerSlot接受currentGeneration；`:85`请求附旧generation。
2. `apps/web/src/host/worker-request.ts:54`／`:55`只核对requestId和**response.generation**等于旧请求generation，成功后`:58`resolve。它不验证**response.nextGeneration**。
3. `worker-host.ts:93`调用generation(nextGeneration)，而`:41`的generation仅要求正安全整数，未比较currentGeneration。
4. `worker-host.ts:356` load直接赋currentGeneration = restored.nextGeneration并生成新baseline。因此nextGeneration等于旧generation或小于它但仍正整数的异常restored响应会被采用，不能据“旧请求匹配”核销这一跨局隔离守卫。

代表性静态负例：保留`worker-host.test.ts:97`的合法snapshot／requestId／response.generation=1，只将nextGeneration=2改为1；请求仍匹配，generation(1)通过，helper将返回1。针对回退可用currentGeneration=4、response.generation=4、nextGeneration=3。当前文件`:97–112`只有正例；全文检索没有定位非推进generation的负例。这是代码推导，**未运行该负例**。

反证与实际影响限定：`apps/web/src/host/wasm-session-slot.ts:64`真实成功restore仍generation += 1；`wasm-worker.ts:239`返回slot.readGeneration。因此没有证据称正常受控Worker输出非推进generation，也不称正常默认局资产已被恢复错。遗漏是历史已明确的协议防御及对应边界测试；处理应在host采纳新baseline前拒绝nextGeneration <= requested/current generation，不能以忽略坏响应或继续用旧baseline静默fallback替代。

旧总账检索未找到nextGeneration／non-advancing守卫项；G04处理暂停／继续重送旧baseline，G01–G05是远程协议及连接，均不是本项。交主控独立复核是否升格为正式G；本分工不修改产品。

## 日历、三宿主日界与其他候选反证

- 官方年度覆盖落入模拟回退：`packages/engine/src/calendar/holidays.rs:76`找到coverage后仅假日返回，`:88`仍fallback；旧G15已有，不能当本批新增。
- 自然日公开可见性：`packages/engine/src/session.rs:2030`／`:2043`用当前civil date零点查询；日终18:00公布后时钟已到次日，报告可见。不是宿主用墙钟推导披露时点。
- 三宿主帧、CivilUpdate与公共披露事件接线已有；原Task29:122“Server discards events”已经失效。固定倍率聚合／继续旧baseline／重连／pull等旧G项仍独立存在，不因本批事件接线核销。
- 公共period跨宿主共用engine修复已落实，不再登记“WASM修了而Server/Desktop未修”。公开响应严格parser也已落实，不把Task30:9旧residual重复登记。
- 固定集团公开报告、经营付款、散户基本面等不由这三份历史review增加范围，保留总账G28／G35等已有项。
- 除C29-01外，本批未确认新增遗漏。源码阅读与历史APPROVE不证明绝对无边界bug、完整长验收或最新全宿主环境通过。
