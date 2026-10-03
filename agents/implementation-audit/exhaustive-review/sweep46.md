# 历史实现穷尽复核 sweep46：公司模型交接、issues 与 learnings

## 全文读取及基线

- 产品按父任务指定 `b76ece3`；merge产品diff为空。承接根AGENTS/principles读取；本批仅新增工作记录，不改产品、Git或历史文本。
- 连续全文读完 `docs/superpowers/specs/2026-09-11-company-information-handoff.md` **85行**、`2026-09-13-company-information-issues.md` **1391行**、`2026-09-13-company-information-learnings.md` **1499行**，共 **2975行**。
- handoff 1–85完整；issues按1–250、251–500、501–750、751–1000、1001–1250、1251–1391读取；learnings按1–250、251–500、501–750、751–1000、1001–1250、1251–1499读取，最后1488–1499另读补齐输出包装截断的几个词。历史乱码保留，不能凭猜测补写损坏原句。
- 未运行测试、GUI、WASM构建或规模/统计验收。旧“passed/APPROVE/blocked”只作为当时证据，当前事实以caller和后续正式决定为准。
- 未确认总账之外独立产品G。现行缺口落入G06–G09/G15/G16/G28/G35–G38和Q02/Q03/Q11；旧8MiB限制、过渡恢复、公开null/date、镜像增长、duration==0、重述双计和notes静默溢出都有后续反证。Playwright `COREPACK_BIN`历史要求与当前bare pnpm差异作为待定工具候选，未直接立G。

## handoff 逐章

| 原文章节、行号 | 当前状态／依据 |
| --- | --- |
| §1当前状态8–18、本轮19–23、复核24–25 | 26/46、944 tests、旧分支/HEAD只属2026-09-11快照；后续代码不得按这些计数判是否完成。V删除由现行compute单一MarketView、无evolve_v权威路径承接，不恢复旧隐藏V |
| §1a 27–38 | Task27 WIP已经后来收口：`session/persistence.rs:258`校验schema/政策，936–939行公司/披露/个人/计划校验；`session.rs:2926`起覆盖保存权威公司与个体状态。旧UTF16/happy证据修复不是当前存档未实现证据 |
| §2 40–51 | 27→42/F1–F4是历史实施与验收流程；当前总账已单列长期/三宿主/GUI/统计证据，不能从任务勾选或构建artifact推导全部真实验收完成 |
| §3 53–59 | 换机环境、旧944测试计数与历史并行资源安排；现行 `scripts/wasm-build.sh:5`委托frontend-build `--package-manager corepack`；核心产品仍同engine，不需要按旧恢复步骤执行checkout/push |
| §4 61–71 | 64行“恢复重放经营/adopt_all_pending”已替换；65行独立RNG仍有效；66行严格存档仍有效；67–69行PS/LSP/generated owner/E目录是历史过程；70行旧before不可改但旧采集器已获批退役；71行错误事件不等于自动失败 |
| §5 73–79 | 75行已更新撤销公司256等门槛；保留 `persistence.rs:944` 512MiB解码边界。29公共DTO已实现，`company/query.rs:17`最大page100、`information/queries.rs:26`稳定ID游标。现行权威事件 `session.rs:285/294`，日终在2143/2164/2172行生成 |
| §6 81–85 | 历史Git与用户改动保护记录，不是新增产品需求；本批无Git写操作 |

## issues 逐章台账

以下相邻小节合并仅为避免重复事实，列出的每个原文章节均已全文读取。I代表 `2026-09-13-company-information-issues.md`，L代表同日learnings；路径均从仓库根起。

| 原文章节及行号 | 现行判定与代码证据 |
| --- | --- |
| Todo4 7；Task39 14 | 文档检查历史记录；8MiB blocker已变化：`apps/server/src/routes.rs:41`当前MAX_LOAD_BODY_BYTES=engine MAX_SAVE_DECODE_BYTES+1MiB，engine `persistence.rs:944`为512MiB。`apps/server/src/lib.rs:109`接Axum body gate；不再声称20k/100k必因旧8MiB门禁不可load，也不宣称已实测大档远程成功 |
| Task35 incomplete18、completion22 | privileged adapters已接：Server `routes.rs:567` debug+feature guard，575行鉴权后actor查询；Tauri `lib.rs:193`同guard，198行generation校验。旧“未实现server/desktop”排除 |
| Task30 follow-up27、null39、date48、central57 | WASM public `lib.rs:33` serializer显式missing-as-null，406/418两公共query分别412/422行使用；`company/query.rs:493` central period_end_date，369行window_end；旧undefined/month-only/只WASM修日期均已核销 |
| Task32 blocked65 | 历史GTK缺环境，后续L34明确native resolved；真实注册invoke/Wayland截图仍是验收边界，不当产品没实现，也不把MockRuntime actor等同GUI/IPC全过 |
| Task29 re-review78、Task33 observation84、Todo2 blockers91、downstream97 | 当前 `session.rs:2110`封账，2143/2164行披露，2172行CivilDateAdvanced；Server `actor.rs:1203`循环日结并用update，1219行rollback；Web coordinator `company-query-coordinator.ts:17/40/48`统一normalizer，当前HostUpdate按统一protocol接线。旧缺模块、丢civil vector、metadata修复历史不自动重开；当前host问题仍按G01–G05 |
| W1Task1 107、Task3 133 | 独立复核早期基础设施blocked/LOC/旧clippy/before记录：handoff24–25及后续review已收口，非今天欠产品；旧采集/逐字冻结回放已 `docs/test-cleanup-checklist.md:60/81–91`正式退役，历史证据不能恢复成必需工具 |
| W1Task2 155、二次取证224 | 法源blocked和后续6项verified属于依据债；不虚报获取2006CAS正文，不据现存游戏假设要求偷偷实现真实税/利率。`docs/company-accounting.md:60`仍明确CAS18阻塞，与版本化游戏政策分开 |
| Task21 190 | Expired不可达旧缺陷已有明确fix记录；现行 `plans`状态机与production应用存在，不把历史自审漏检重开；长期history复制仍G16 |
| Task4乱码243 | 全段已读取；可信可读内容HKO2051星期错误不影响Gregorian计算：`calendar/data/mod.rs:52` LUNAR_WEEKDAY_DEFECT_NOTE保留，53行说明engine自算weekday；官方覆盖非假日fallback问题仍G15，不将HKO来源错误当当前weekday算法错 |
| Task6 268、review285、Task5 follow-up300 | Books唯一post_batch/恢复Journal重建与守卫已有；旧单文件LOC/先在lint/补review背景不新增功能。公开保存quiet-point与日终宿主存档分开，见sweep28 |
| Task7 305、Task17 326、Task8 345 | 公司装配/权重pure primitives已有；账号身份与实际分析能力生产接线缺仍G07。工商经营payment/depreciation/tax闭环仍G35，不能以纯Books processing金样核销 |
| 乱码handoff370及子节372/376/381/389/400、用户延期405 | 历史进度/并行owner/基础设施/复核延期，原文损坏未修；不根据未知字句发明新产品条款。后续handoff/review结论及代码优先 |
| Task8 observations410 | zero加工费、空税、days0还本、零息金样为非阻塞测试建议；available_credit只读None已明示接受，不自动当禁fallback异常。真实处理器已有，不要求为覆盖建议增加产品功能 |
| Task19 422、binding review484 | 指标pure接口/个人memory已有；绑定积压归后续Task29且当前generated目录存在。`record_public_history_read`消费未接仍Q02；不把测试missing binding的旧运行状态当今天CI必红 |
| Task9 448、review504 | 银行ECL、原生流水/存贷计息已有；微分支测试建议不等于生产无处理器；银行税/复利/罚息/ECL折现等已明示简化，不超范围恢复 |
| Task20 531 | dated经历primitive是有意过渡态；今天生产仍legacy消费属G08；fixturedated writer可用不核销生产writer。feature optional反馈已由严格当前schema边界核对，不能恢复旧extraction hash |
| Task11 569 | RE资本化游戏假设、单科目、现金流分类/禁VAT/无现售等明示，反向冲击和多行业session缺仍G36；数学孪生待统一仅重构建议，非独立漏功能 |
| Task10 615、REJECT648 | GMM五简化的docs/fixture缺登记已修：`docs/company-accounting.md:93–107`，policy-sources.json542行；不新增获取现金流/贴现结构真实业务。旧上下界用例建议与配置守卫分开 |
| Task12 664、REJECT750、F3 780 | duplicate往来申报已 `consolidation/eliminate.rs:36–51`类型化拒绝；政策登记 `company-accounting.md:49/51`、fixture557行已接。**F6账面上界缺仍G28**；不要因双抵已修宣称超账面申报也已修。集团单层/精确bp/无股权投资抵销是批准简化 |
| Task14 709、REJECT784 | pacing docs/fixture已在 `company-accounting.md:128`、fixture574行登记；**F-O1行业不适用shock→公告仍G36**；F-O2 duration0已 `operations/config.rs:156`、180行拒days<1，`core.rs:191`装配调用；F-O3 mirrored增长已 `session/company_operations.rs:110` prune，`session.rs:2123`生产日结调用 |
| Task13 816、REJECT849 | restatement双计已 ClosingEngine持久化 `accounting/closing/mod.rs:99` register，376行向每次generate传scope调整，`closing/save.rs:21/32/45`序列化/恢复；notes静默溢出已 `reports/notes.rs:193` `closing.sub(movement)?`。report simplifications `company-accounting.md:135`、fixture606行已登记。BusinessKind粗标签被本文明确接受顺延，不立当前强制迁移 |
| Task15 881 | 月/年封账已有 `session.rs:2184` close_accounting_periods，2110行日结调用；默认industrial以外books_mut仍 `operations/config.rs:43`不可达分支，四行业交付仍G36。APPROVAL_HOUR/不可达常量合法守卫/AccountingPolicyRef只有chart版本属于旧范围与Q03，不因字段名缺失判冻结失败 |
| Task16 926、review962 | 公共曝光与本人获知有 `information/acquisition.rs` /npc_view；重复获知EarlyRead语义及多公司金样建议非缺实现；APPROVAL_HOUR登记有后续正式report schedule fixture502行，不自动要求额外版本对象 |
| Task25 REJECT982、reverification1055 | discovery参数先缺登记后明确on-branch修，范围条件已注册；本轮不回退到事故a885ac1或另补错误分支。注意力共享公共信息不等于本人读，当前生产获知由机构root链处理 |
| Task18 1011、Git事故乱码1049 | five-yearDCF仍G06；中期不作为更新仍G09；Correction/CreditDefault primitive与production caller关系仍Q11。事故reflog/禁amend是历史流程，无当前Git修复授权 |
| Task23 1064 | 旧卖250/可卖150guard漏已修并复核；quote policy只输出动作，实际撮合仍engine；不将配额/保护限价/recovery建议当新执行路线 |
| Task22 1084 | duplicate PlanId、lot100 guard、半偶边界已记录修复；现行 `decision_chain.rs:972/1236`全部ExistingPlan分类仍G38，不把内部账户软预算优先级当跨账户优先 |
| Task24 1102、finish1123 | 外部PlanBook/即时执行为旧过渡接口且后续清除；linked-child真实partial restore有 `pipeline/continuous_tick_transaction_tests.rs:521`。同账户同股重复计划旧ordinary materialization陷阱已纳入现行P3/P4，当前不能根据退役API重开 |
| Task26 1150、修复1203 | 保存重放/个人状态复位已Task27覆盖， `session.rs:2926`恢复权威个人域；市场价格笼子测试仍 `tests/market/price_limits.rs`，不恢复V测试。default只工商与books_mut仍G36，default税/经营成本版本化游戏参数已有，不要求真实市场校准 |
| Task27 1240、Task28 1246 | 新存档已收口；Task28初始REJECT后来APPROVE不能照搬；当前same report/partial restore/year-test源码详sweep28，长验收/准确双构建parity仍待证 |
| Task29 1253、Task30 1261 | 隔离index和600秒构建当时环境记录；current普通10s/长300s纪律优先。coordinator严格normalizer已接，WASM候选先替换和generation防迟到在宿主代码，不凭历史remaining gap重开 |
| Task31 findings1267、observations1302、privacy repaired1314 | current server `actor.rs:1124` checkpoint，1203行civil，1219行rollback；`actor.rs:245`显式只选player0。旧full accounts泄漏、swallowed civil已反证；restore/resync现行问题逐项归宿主总账，不据旧元数据类型名必须保留旧PublisherFrame |
| Task32 blocker1276、residual1288 | currentEvent285/294、WASM public406/418、Tauri public lib159等已接，旧缺shared API核销。registered invoke/GUI仍需实测，不能代MockRuntime通过 |
| Recovery overwrite1295 | 仅历史notepad事故；本轮不修乱码/恢复旧Git，保护原历史证据 |
| Task34 blocked1321、retry1327、final1331、date修1340 | publicnull/date central已修， `components/StartDateInput.tsx:24`直接value无2030fallback；图表TV水印是scope外当时视觉观察，未有现行规范要求删除，不立G；真实视口/视觉证据仍需验收 |
| Task36 missing1351、implementation1357、clock1376 | source causal ledger已实现， `session/causal.rs:16` civil由28行observation_civil_instant，`observation_clock.rs`统一午休真实时钟；causal aggregate43行对ObservationRestart明确错误，不伪造前史。**DEV简化trace真实order关联仍G37**，不能拿causal collector存在核销另一输出链 |
| Resolve-blockers evidence1387 | actor/helpers≠注册Tauri invoke；oldPNG0字节、Wayland截图不能当GUI通过；总账验收边界承接，不新增代码G |

## learnings 全部章节映射

| L章节、行号 | 当前判定与对应issues／生产链 |
| --- | --- |
| Todo4 7、scale18 | 8MiB旧正文被current512MiB门禁取代；C06真实校准被ADR0023后续排除。四行业10年484MB样本不等于生产会话四行业已接，G36仍存 |
| diagnostics isolation24、native blocker30、resolved34、stale/Wayland39、prereq44 | collector唯一feature已有；三宿主额外debug guard保护API不是偷用debug替代collector。Tauri `actor.rs:78` closure generation helper、1017行生产caller先guard再private read，1310行计数negative测试。env早blocked后resolved，GUI矩阵仍不外推 |
| Worker49、nullable61、old date70、Tauri80 | currentWASM33行explicitnull、enginequery493行central date、serde-normalize.ts:64 accountMap、三宿主generation/candidate restore对应I27–65。old只WASM转换已被L1415后续统一决定核销 |
| shared state100、restore gate106、reviewrepair111、metadata116 | `company-query-coordinator.ts:17/40/48`复用strict normalizer；`serde-normalize.ts:85–88`恢复数值keyMaps；新protocol已替换旧cachedMetadata字段，不能要求老PublisherFrame结构复活。G01–G05现行问题另核对 |
| before121、refactor150、official198、supplement223 | old80min采集与WASM/Linux环境不能扩当前deadline；旧VError/before原记录不篡改。纯移动结构与法源方法说明，不新增业务承诺；正式CAS缺口仍诚实blocked |
| plans245、fix293 | 状态机/guard可达性正确演进；PlanBook权威历史和活跃索引已有，复制目标仍G16；显式长期expiry不等于普通tick遍历所有旧计划 |
| calendar乱码315 | 完整读，HKOweekday错误按代码说明核销，G15官方覆盖替代缺仍保留，2099后制度不擅定 |
| accounting337、civil378、company430 | Books纯原子处理与自然日/交易日分离已有。civil早期“step未动/过run需收紧”已由ProtocolSession生产civil_day_ready与actor循环替代，不要求每tick都伪造CivilUpdate；新局/恢复严格schema不保留旧default兼容 |
| profile470 | 五路权重派生和独立seed已有，生产散户分析缺G07。Bank/Insurance方法EquityRoe为显式游戏规则，不要求每户随机不同 |
| industrial504 | 纯会计处理器已有；生产折旧、所得税、债务支付闭环仍G35。RHE/ACT孪生和BusinessKind粗标签属已登记tradeoff，不为追字面单一函数重构 |
| technical550 | SMA/RSI/ATR个人有界观测已有，成交volume0不补伪样本；public history read留痕未接Q02；旧“日K360”背景过时，不恢复删除历史。MACD/KDJ batch不同承诺另G17 |
| bank601、experience649、RE696、insurance746 | 四行业pure内核存在；金融/地产已批准简化不自动升级现实完整准则。Retail dated/衰减primitive仍G08生产遗漏，机构ADR0026已完成不重开 |
| consolidation790 | 确定scope/少数/权益/现金纯五产物已有，但现行集团session披露仍G28；重复decl守卫已修、F6账面上界仍G28。精确bp/单层/亏损销售拒绝现行简化 |
| operations844 | 四行业flow/config/scheduler存在，durationguard/mirrored prune后修已证；公司行业过滤shock缺G36，信用恶化记录不等于违约不能转CreditDefault |
| reports902 | report五产物、双口径更正已有，持久restatements修复后不重复计入未来NI；notes checked-sub传播已有；合并restatement不支持已登记，不强制新增 |
| information960、acquisition1026 | 公共不可变ID + as_of guards与个人首次获知钉版已有；月/年封账current生产caller已接工业，非工商G36；APPROVAL_HOUR常量与法源资料边界待Q03说明，不凭结构名称要求全政策包 |
| watchlist1075 | discovery权重、protected持仓/活跃计划不驱逐、attention RNG独立已有；保护不是无限全局配额，曝光不是已读。需生产root实际消费证据，当前链已定位 |
| beliefs1087 | annual个人信念链实际接；DCF错误G06，中期G09；Correction/CreditDefault Q11；同消息不同先验controlled现行已有。L1130附近旧DCF191648179金样不是正确逐年PV，不能用旧脚本自洽抵销G06 |
| urgency1144、allocation1164 | Patient/Normal/Urgent保护动作已有；卖数量上界补guard已核销；soft cash无预计卖款输入。ExistingPlan与新机会实际分类仍G38，不重排不同账户撮合 |
| planexecution1182、finish1203 | 旧即时public执行接口后续退役，现行tick真实partial restore已接；本体/parent link同一真相。不能因旧test目标名不存在回退旧API |
| decisionchain1214 | 共同V彻底删除；公司assembly成本/flow与seed独立；当时个人cost unavailable已被ADR0026后续个体机构行为改变，不能沿旧句重开机构经历。恢复过渡已Task27替换；G07/G08/G09/G28/G35/G36/G38仍current |
| Task27 1275、Task28 1287、Task29 1299、Task30 1310 | 保存strict/queryDTO/generation后续接线；RuntimeResource旧配额取消；Task28已有修后源码，fresh长场景/精确parity仍待证。详sweep28 |
| Task31 findings1322、repair1364、privacy1383 | 先旧REJECT再repair：currentactor checkpoint/rollback，publicSnapshot player0，currentbody512MiB；旧100000数组配额被ADR0019撤销，深度64guard仍保留。不得恢复任意集合数量限制 |
| Task32 prerequisite1332、resume1341、Task29 events1351 | sharedEvent序号与civil/disclosure权威生成已有；server/desktop直接引engineprotocol。仅注册IPC/系统运行环境仍需验收；generation strings保持无损 |
| Task34 UI1394、old date1401、central1415 | sharedCompanyPanel/Redux来自严格公共DTO；`company/company-presentation.ts:45`BigInt金额，100行NoPriorYearHistory明确不可用；centraldate/null已修，旧phase结论按后追加核销 |
| Task34 browser1427、controlled-date1441 | date value修已代码确认；Corepack webServer要求和当前bare pnpm不符，候选见下；DOM几何/scroll/focus证据不能从文件名推当期视觉通过 |
| causal discovery1453、ledger1459、clock1475 | fullcausal ledger有真实来源IDs，与缺tradepublicorderID不矛盾；restore不能虚构hist，civil使用统一observation时钟。G37仍是简化DEVtrace空events独立漏接 |
| Git paragraph1487、pinned1489、Corepack/clippy1495 | oldcheckout/clean状态及固定toolchain统计仅历史；corepack-pnpm.sh:34显式corepack，wasm-build.sh:5已route统一frontend-build；currentPlaywright仍需单独核对。lint机械修不影响T+1/RNG，无需恢复旧warning |

## 新候选与反证

| 候选 | 最终归类及依据 |
| --- | --- |
| 默认20k save远程超过8MiB必失败 | **旧事实已核销**：currentserver41行512MiB+1MiB与engine解码同上界；此处未运行大payload测试，只确认旧固定8MiB不成立 |
| 封账更正双计/notes溢出/zero duration/镜像历史无限增长 | **已有修复反证**：persistent RestatementRegister、checked-sub ?、config days<1+buildcaller、日结prune四条链均有，不新增G |
| 非工商books_mut/未过滤行业shock/集团F6金额上界/散户dated/中期/机会分类/DEV订单关联 | **并入已有G36/G28/G08/G09/G38/G37**，不按历史多处描述重复计数 |
| Correction/CreditDefault、历史主动读取、政策冻结对象 | **Q11/Q02/Q03**保留待明确，不把信用恶化、共享cache构建或无同名RegulationProfile自动判缺必做 |
| 工商BusinessKind粗标签、RHE和ACT函数孪生、bank税/insurance acquisition现金流/RE VAT | **已登记简化或顺延设计**，不新增功能；所有权优化与真实生产闭环缺口已另列G |
| oldTask28/GTK/Wayland/注册IPC/K7长场景/statistics | **验收证据边界**，不以source存在宣称通过，也不把历史环境blocked改成产品没实现；后续最新证据须由总验收汇总 |
| Playwright webServer不支持COREPACK_BIN | **待定工具候选**：L1429明示must not assume pnpm on PATH，current `apps/web/playwright.config.ts:19`仍三次bare pnpm且无COREPACK_BIN；wasm-build已使用corepack。正式ADR0027允许测试build路径、要求固定pnpm构建但未承诺每test config无shim可执行。历史learning可能被最新构建环境取代，暂不自动新G；需要核对E2E正式launcher是否保证PATH/shim以及该历史配置承诺的现行适用性 |

本批没有产品行为变更；所有A股简化及单位按当前正式文档处理，不添加资金流/真实行情，不修改乱码历史。父任务统一完成独立diff复核。
