# sweep36：company-information-npc-intentions 三份 notepad 全文复核

产品审计基线 `b76ece3`，checkout `4ad5a2e` 为审计 merge，产品相同。仅新增本文；未修改产品、未执行 Git 写操作、未运行测试、编译或长验收。本文的“已有”是源码/owner/caller 存在与接受集检查，不将历史绿色输出冒充当前运行验收。

## 全文范围与可读性

| 文件（均在 `.omo/notepads/company-information-npc-intentions/`） | 全文行数 | 连续阅读 |
| --- | ---: | --- |
| decisions.md | 7 | :1–7，只有 scaffold，无实质决策 |
| issues.md | 1373 | :1–300、301–600、601–900、901–1150、1151–1373 |
| learnings.md | 1474 | :1–300、301–600、601–900、901–1200、1201–1474 |

第一次三文件合并输出被截断，已以上述连续切片重新阅读 issues/learnings 全文，不能用第一次截断输出宣称读完。共 2854 行。AGENTS/principles 已在 sweep18 读完。

issues:231–254、358–395、1037–1040 及 learnings:304–324 原文件存在不可逆替换字符/乱码；可辨识的代码名、任务号、日期、路径与数值已核对，不猜造不可读中文。这些段主要记日历/交接/Git事故；另有清晰的同主题正式文档与后续记录，按可读记录映射。不覆盖或改写历史 notepad。

## 全文逐主题映射（所有章均归入下表）

I 表示 issues.md 行号；L 表示 learnings.md 行号。每行包含该任务全部初记、review、修复补记与工具/文件布局观察，范围内不只读取未勾条目。下表中的既有 G/Q 引用 `implementation-audit-2026-10-02.md`。

| 原文章/起始行 | 当前 owner/caller、状态与排除依据 |
| --- | --- |
| decisions:1 | scaffold，无新增能力条款。 |
| Task39 I:7；L:7 | 历史 20k/50k/100k 保存体积与四行业3652日大样本为当时测量，不保证现行吞吐。旧8MiB问题已取代：`server/routes.rs:41` 使用 `MAX_SAVE_DECODE_BYTES+1MiB`；`session/persistence.rs:944` 为512MiB。ADR0019明确移除任意条数配额。性能、最小存档与跨宿主实际验收继续是既有证据边界，不能只凭旧尺寸断言当前Remote不可部署。 |
| Task35 I:11、15；L:13、19、23、28、33 | 早期Server/Tauri缺adapter与GTK blocker后续有明确源码/验证记录。当前 `server/actor.rs:1441–1454` 先比generation再读diagnostics；`desktop/actor.rs:78–91、1017` closure先守卫；:1339/:1362是feature条件测试入口。普通构建collector隔离存在；真实诊断订单关联仍为既有G37，不能因路由存在将它核销。Xvfb/Wayland记录是历史环境实测，不是本轮GUI证据。 |
| Task30 I:20、32、41、50、1249；L:38、50、59、1299、1390、1404 | 非执行脚本/PATH/并发TS缺模块属当时工具/共享树限制。public None输出已有 `web-wasm/lib.rs:28–40` explicit-null；报告日期统一改在共享engine DTO，旧WASM-only补丁已取代。`web/host/serde-normalize.ts:20、24、64` 负责严格报告解析与恢复Map转换。Worker真实生产边界不能由旧rust JSON证据代替；当前本轮未重建WASM。 |
| Task32 I:58、1264、1276；L:69、1321、1330 | 共享EngineHost query/Event先决条件已有；generation为字符串，Tauri query与restore委托engine。`desktop/actor.rs:993–1020` 有civil/report/by-id/diagnostics命令。早期“engine没有civil事件”已被Task29修复，不能重复登记；GTK系统依赖是证据环境约束。当前固定速率聚合缺口见G19。 |
| Task29 I:71、85、1241；L:1288、1340 | 共享civil/disclosure事实已有 `session.rs:2073、2100` 与 `session/protocol/civil/session.rs:283`；server `actor.rs:1204` 消费civil update。早期server丢events与缺query前置已不成立。TS生成及严格save parser已有；临时Git index/并发模块/format差异是历史验收条件。 |
| Task33 I:77；L:89、95、100、105 | 公司协调器、Redux cache、public DTO解析与metadata一致性已有。`web/host/company-query-coordinator.ts` 委托normalizer；`serde-normalize.ts:20、24` 公共检查；Remote生产更新委托publisher state。反向coverage、丢civil metadata、旧缓存standalone event为已记录修复；不能重新假定当时219/220失败仍存在。Remote恢复/继续重baseline边界仍为G04。 |
| Task1 I:95；L:110 | before基线fixture/runner存在于 `engine/examples/baseline_fixture.rs`、`scripts/simulation/baseline-run.mjs`；stdout摘要/错误计数为观测，非保证交易必持续。旧80分钟/单seed500秒预算被现行AGENTS与K7五分钟共享deadline取代。库/脚本布局与Windows编码/PATH记录不等同产品漏实现；历史独立复核欠账仍须通过证据记录核对，不能用本轮静态阅读宣布历史每个diff获审。 |
| Task3 I:121；L:139 | 纯移动模块接缝存在，session/strategy/behavior已进一步重组；旧“同名文件缺失”与字段可见性变化不算功能遗漏。`session/attention.rs`、views、self_views与snapshot仍承担对应职责。LOC/SIZE_OK与早期clippy属于工程债观察，不证明现行漏接或当前lint红。 |
| Task2 I:143、212；L:187、212 | policy-sources fixture/官方依据登记存在；先blocked后verified的CAS25/33/37/23等必须用最新条目。税法/2006批准则与官方休市取证缺口保持正式docs诚实边界，不伪造现行官方规则。出版物/API/PDF获取方法、Git事故无产品能力要求；沪深假日official输入边界已有G15。 |
| Task21 I:178；L:234、282 | PlanBook/state/revision仍是单一计划状态机；`plans/revision.rs:62–71、300`保留allow_beyond_horizon显式豁免，Expired/补日终不再因守卫互斥死亡。Completed来自真实fill，其他终止携reason；`plans/mod.rs:397` 批量应用活跃事件。旧容量/序列化锚是历史事实，不要求恢复旧字段形状。 |
| Task4 I:231；L:304 | 乱码段可辨识为HKO/calendar/data/policy及2051weekday瑕疵；现行TradingCalendar含runtime边界与模拟假日来源。官方calendar输入缺口已G15，默认official为空不能据此说默认局触发。模块拆目录/跨AGENT共享lib/rustfmt/平台乱码为过程记录。 |
| Task6 I:256、273；L:326 | `accounting/mod.rs:84、98`唯一post_batch验证/原子过账；:191 serde重放账本。i128 AccountingAmount、字符串serde、FractionUnits、closed period与派生ledger存在。`account_net_debit`未知科目投影0已在原文:267作为非阻塞可接受读语义，无新消费者要求，不新增强接口。 |
| Task5 I:288；L:367 | `session/civil_clock.rs`自然日与交易日分离；`session.rs:2073–2123` now具备经营→封账→披露及镜像裁剪。旧step完全不动/月底尚未封账属早期过渡；现行caller见civil update及宿主actor。同步/失败恢复仍按真实测试入口核对，不要求恢复旧单文件尺寸。 |
| Task7 I:293；L:419 | `company/spec.rs:73、99`CompanySpec及集合校验，issuer映射/股本约束存在；company opening/contracts与外部对手方隔离存在。默认全部Industrial、无法四行业自定义是G36，不将已有四行业域内unit实现说成会话实现。 |
| Task17 I:314；L:459 | analysis_profile/factory_profiles权重归一、稳定奇偶、固定主导风格已有；`session.rs:1893、1897`独立RNG导出profile/个人假设。只对机构走生产完整belief链为既有G07，不能因库支持Retail权重核销。expect数学上界/零权重不抽为已有测试契约，未证明可触发新panic。 |
| Task8 I:333、398；L:493 | 工商共享子账、VAT、MWA、折旧、ECL、ACT/365F域内实现已存在；真实运营折旧/税/债务支付未完整接线为G35。`industrial/{production,purchasing,capex,sales}.rs`仍用粗BusinessKind标签；I:860已明确“报表不消费kind，可接受顺延”，披露当前用Shock而非这些粗标签，不升级为漏实现。零加工费/零税/零天还本/零金额计息是原独立复核非阻塞测试增强建议，不能伪称尚缺实现。 |
| HANDOFF I:358（子章360、364、369、377、388）、393 | 乱码保留；可辨识的交接任务、分支、独立复核延后与sourceowner冲突属历史工作编排。后续review章覆盖各任务结论，不能仅因为交接当时8/46认定如今只完成8项。 |
| Task19 I:410、472；L:539 | 技术SMA20/60、Cutler RSI14/ATR14、15有效日K、无量日过滤内核已有；Root五路信号消费technical。PublicHistoryRead记录尚无生产caller为Q02；不能伪造每次cache重建就是主动历史阅读。旧TS文件未提交已经由正式生成目录统一收编，不按当时24文件缺失重开。 |
| Task9 I:436、492；L:590 | bank/domain的存贷、计息、三阶段ECL、核销/回收/报表分类存在，`bank/ecl.rs:188、226`逐次校验并过账。全部Operating、存款到期停息、逾期贷款续合同息是登记简化；非工商会话封账不支持归G36。本金收回成功/零差重估/365天边界是历史测试建议，不构成单独产品缺口。 |
| Task20 I:519；L:638 | `experience/feedback/{lifecycle,inputs}.rs`dated writer/read原语存在；散户legacy生产仍未完整消费这些新读写缝，已G08。原I:539“有意过渡态”不可当现行完成；机构经历后续ADR0026已实现，不与散户串账。 |
| Task11 I:557；L:685 | 地产开发/预售/交付/资本化双余数域内实现存在；CAS17受阻采用版本化game-assumption，正式 `company-accounting.md`登记。只成本不数量的开发库存不能机械要求复用InventoryLedger结构。非工商会话接线仍G36；税/现售/许可/信用期等明确简化不擅自恢复。 |
| Task10 I:603、636；L:735 | GMM/CSM/亏损组/LRC/LIC/单利贴现实现存在；最初REJECT的缺docs登记已在 `company-accounting.md:95–108`登记获取现金流/利率/亏损列报简化。InvalidDiscountRate上界测试建议不构成算法漏实现。非工商完整生产闭环同G36。 |
| Task12 I:652、738、768；L:779 | consolidate算法/归母少数拆分已有；`consolidation/eliminate.rs:33–58`先拒绝重复成员对申报，原复用首镜像双抵已修。`company-accounting.md:49–51`合并六项简化登记已有。等额申报账面上界与公开合并生产链仍G28；禁止把两者与已修重复申报混报。 |
| Task14 I:697、772；L:833 | company/operations顺序advance/due/shock/flow/interest实现存在；`operations/config.rs:159–183`期限≥1守卫修复DueSkipped零期限脚枪；`session.rs:2123`/`company_operations.rs:110`已裁剪mirrored集合。银行/保险不适用Shock仍进公告链为G36；获批仅记录信用恶化与无现售/不延长完工按docs:132已登记，不能再当无依据缺陷。前史账套as_of无统一访问器是原caller契约，不凭此增加新校验API。 |
| Task13 I:804、837；L:891 | 五报表、重述底稿、封期/更正已有。`closing/mod.rs:99、273、376`保留restatements，`closing/save.rs:21–48`入档恢复；`reports/notes.rs:193`期初sub用?，原溢出吞0已修。docs:137–148登记七类简化。重复年度利润旧REJECT不再成立；跨集团公开仍G28。 |
| Task15 I:869；L:949 | PublicLibrary/排期/严格shape/version/不可变id查询存在；原books_mut不可达由Industrial接线 `session.rs:2110、2184`解决，其他行业仍G36。批准08:00常量 `information/publication.rs:27`存在，old docs折叠登记为文档口径观察，不构造新功能。 |
| Task16 I:914、950；L:1015 | `information/acquisition.rs:141–159`属主、公开时点、幂等顺序保留首次获知；Root `roots.rs:360`真实record_acquisition。NPC ctx只已知publication，未读库查询不推进个人状态；多公司happy/重复早读组合是历史非阻塞测试建议。 |
| Task25 I:970、1043；L:1064 | discovery权重与watchlist保护/淡出已有；Root accepted注意力真实获知。最初五参数无policy登记随后独立reverify翻APPROVE，事故a885ac1不能作为有效源码基底，b2d88a9为历史正确修复。held pool过滤与曝光2日caller存在，不擅自修改60%/70%继承抽样。 |
| Git事故 I:1037 | 原乱码段有reset/amend事故与恢复说明，仅历史协作警示，不是产品契约；本轮未执行写Git。 |
| Task18 I:999；L:1076 | 个人assumptions、年度facts、三估值方法、cause/期限/信心更新原语存在；Root `roots.rs:370、440、456`仍Annual筛选及NewMaterial/HorizonExpired。中期补充G09、Correction/CreditDefault分发Q11、DCF逐年折现G06均已登记；不能用原错误191648179金样宣称五年DCF正确。 |
| Task23 I:1052；L:1133 | urgency/quote_policy/validation及Patient/Normal/Urgent/不可撤Keep已实现；原150可卖放行250的守卫已有修复记录与对应库测试。Root/执行真实route连接；不扩为跨账户优先。 |
| Task22 I:1072；L:1153 | allocation/candidates排序、重复PlanId守卫、100股单位、半偶数值已有；真实请求category均ExistingPlan仍G38。原i128溢出经复算不可达，拒绝伪造边界；cash/reserved资源来自本人，不用预期卖款。 |
| Task24 I:1090、1111；L:1171、1192 | `session/plan_execution/{actions,routing,synchronization}.rs`共用真正订单/预留/成交；唯一child、竞价续连续/同id认领已有入口。旧“不入档pending/linked字段默认跳过”被现行save契约替代，不能恢复过渡格式。普通策略与linked并行冲突由Task26/现行planroute承担。 |
| Task26 I:1138、1191；L:1203 | 公司新局与独立profile RNG、Root判断、计划报价/执行、日终经营封账披露均接线；四行业不支持G36、散户G07/G08不随机构链落地自动完成。market非V测试恢复在 `tests/market/{main,price_limits}.rs`，不因旧整文件删除事故宣称当前无涨跌停覆盖。共享V删除由ADR0016/Task26取代全部旧要求。 |
| Task26 pending清理 I:1215；L:1244 | **已修**：`plan_execution/synchronization.rs:51–69`原子应用后清pending（含同批完成后的day-end事实），避免迟到终态事实长期增长。不再以旧“保留未知/终止且无drain”状态新增缺口。 |
| Task27 I:1228；L:1264 | K7恢复持久化与验证已有；旧ResourceLimit/固定capacity路径被ADR0019删除，不能要求重新引入任意配额。保存只必要事实、现行独立请求受理规则仍须按总账和最新K7工具检查。 |
| Task28 I:1234；L:1276 | 真实Session场景测试存在，初版错误T+1/域内调用/空diagnostic parity已有最终修复记录。全场必成交从未成为真实资金域保证，零counterparty是合法输出。历史跨线程整局字节一致验收被现行受理轨迹契约细化（ADR0017等）；当前K7旧比较仍G39。 |
| Task31 I:1255、1290、1302；L:1311、1353、1372 | `server/actor.rs:1204、1220`civil失败明确stop，不再warn后继续；generation/public_revision/timeline更新机制存在。WS baseline player-only投影、header鉴权已存在；浏览器query-token链与header冲突是G01。resync/pull/心跳/继续重baseline问题按G03/G04/G05保留。I/L旧100000nested固定quota被ADR0019取代，仍保留64depth `routes.rs:42`。 |
| Recovery I:1283 | append-only误覆盖记录为工作证据恢复要求，无新增业务能力。本轮只新建审计记录，未重写这些文件或假造丢失字节。 |
| Task34 I:1309、1315、1319、1328；L:1383、1416、1430 | CompanyPanel共用Redux、BigInt显示、缺比较Unavailable、日期选择已存在；`StartDateInput.tsx:24`直接value，无2030 fallback。旧报告nullable/date生产阻塞有Task30统一修复。TV watermark与focus/overflow截图是历史视觉观察，不将截图filename或过时错误态当当前失败复现；现行相关UI已G22–G34覆盖。 |
| Task36 I:1339、1345、1364；L:1442、1448、1464 | 私有实际causal collector已有 `session/causal.rs:33、36、110`、`diagnostics/causal.rs:129、141、168`、`causal/report.rs:21`显式restore boundary；`session/observation_clock.rs`统一civil/market时间。不能把旧缺Trade订单ID强制改成公共Event泄露私有信息；collector是独立受feature控制的真实事实路径。Root DEV trace空events仍G37，与causal工具存在不能互相核销。 |

## 本轮候选反证与保留边界

1. **旧100k/8MiB恢复不可部署**：有现行routes/persistence额度及ADR0019反证，排除当前能力缺口；未重新验证大档跨宿主吞吐。
2. **迟到pending无限保留**：synchronization:69清队列；现行saved pending只允许live plans，排除旧状态。
3. **经营期限0导致当日due漏派**：config:180显式 `<1`拒绝，排除。
4. **报表更正后再次计利润/附注溢出吞0**：持久化RestatementRegister与notes:193传播错误已有，排除旧REJECT缺陷。
5. **重复内部往来双倍抵销**：eliminate先DuplicateIntercompanyDeclaration拒绝，排除；账面上界保留G28。
6. **stale诊断先读取私有信息**：Server actor先generation条件，Tauriclosure先守卫，排除；真正订单关联保留G37。
7. **工业粗BusinessKind未偿还**：原I:860–863裁定报表不消费kind可接受顺延，当前披露从冲击状态派生，不证明粗标签已破坏生产报表；只登记历史标签债，不新增产品要求。
8. **未实现中期/散户衰减/四行业/新机会预算**：均已有G09/G08/G36/G38，不重复编号；个体能力身份限制已有G07，DCF数学已有G06。
9. **原非阻塞分支测试建议**（I:402–407、499–511、649、763–764、956–964）：保留为历史测试增强建议，未运行当前测试覆盖分析，不能声称全部已补齐，也不能从“无gold触达”推成“实现缺失”。原文明确不设独立任务。
10. **LOC、LSP、pnpm、GTK、并发WIP、乱码/Git事故**：是验收/协作证据状态，不等同当前产品未实现；须用实际现行工具输出单独验证。原不可读片段已明确范围，不伪造全文语义。

本轮未确认总账之外新的产品实现缺口。已有G/Q与证据欠账继续保留；该结论仅针对这2854行可辨识条款与核对的owner/caller，不是对整个仓库或全部历史绿色声明作无遗漏保证。
