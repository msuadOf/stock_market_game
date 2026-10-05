# Q23：财报公开频率接线

## 提交依赖与限定签核

提交准备以已合入报告种类优先的 `a6ba588a` 为基线。当前共享文件含 Q13、Q08、多人和日终数据库增量；以下是月报任务的依赖边界，不是允许整文件无差别提交的名单，也不是已经通过仅 index 版本编译的证明。源码保持冻结，stage、commit、实际 WASM 与发布 fixture 收口均由 root 统一执行。

可先独立提交同比同窗口修补：`packages/engine/src/strategy/fundamental/facts.rs` 的当前两项增量（`Monthly` 中期种类支持及 checked 附注运动汇总）、`fundamental/mod.rs` 仅 `interim_window_tests` 注册、新 `fundamental/interim_window_tests.rs` 及 `q23-interim-window.md`／`q23-interim-window-review.md`。四项测试使用既有 `Correction` 来源和真实报表生成，不依赖新增月报排期字段；不要顺带加入 `monthly_baseline_tests` 注册。年度基准 guard 的生产 hunk 本身可以分离，但增强短测使用 `MonthlyDisclosure`／`MonthlyReportSchedule`，必须等月报契约批落地后，才与 `monthly_baseline_tests.rs`、对应 cfg 注册及独立记录另作小提交。不能把尚未具备依赖的测试当最小可编译提交。

完整月报契约批须同时保留以下闭环：

- Engine 配置／来源／前史：`information/monthly_schedule.rs`、`schedule.rs`、`mod.rs`、`publication.rs`、`public_view.rs`、`prehistory.rs`；`session.rs` 的必填 `SessionSetup.report_frequency`、validate、模块注册、查询时刻、日终频率读取及各实际公布时点送达；`session/company_assembly.rs` 的两个装配调用。
- Engine 实时／集团／恢复：`session/disclosures.rs`、`intraday_disclosures.rs`、`company_groups.rs` 的前史和 live 排期、成员月封账检查；`session/observation_clock.rs` 的收盘观察时点；`session/pipeline/candidate_commit.rs` 的私有公开与 event key 同批验证；`session/persistence.rs` 的配置排期日终游标严格复核。
- Engine 本人认识：`strategy/fundamental/update.rs` 的两个 `Monthly` 中期路由，`facts.rs` 的中期种类支持（若先行窗口提交已落地则不重复），`session/decision_chain/roots.rs` 的报告分类与真实本人首次观察短测。
- Web 配置／换档／存档：`components/ReportFrequencyInput.tsx`、`app/useSaveCommands.ts`、`app/useSessionHostLifecycle.ts`、`App.tsx` 两布局及换档回填、`config/defaults.ts`、`save/schema/report-frequency.ts`、`save/schema/market.ts`、`save/schema/company/reports.ts`；实际生成的 `types/generated/{ReportFrequency,MonthlyReportSchedule,MonthlyReportPreset,MonthlyReportDelay,SessionSetup}.ts` 必须同批，不能手写类型冒充导出。
- 对应短测／真实 fixture：`packages/engine/tests/report_frequency.rs`、`session/company_groups/tests.rs` 与 `intraday_disclosures.rs` 的 cfg 短测；Web 的 `components/report-frequency-input.test.ts`、`save/schema/{report-frequency,monthly-bindings-contract}.test.ts`、`save/company-schema.test.ts`、`app/save-commands.test.ts`、`app/session-host-lifecycle.test.ts`、`save/save-schema-contract.test.ts`；`save/current-save-fixture.ts` 与真实生成 `save/fixtures/current-schema-save.json`／`current-company-slice.json`，并保留对应生成器的必填配置。
- 正式规则及工作证据：`docs/company-accounting.md`、`docs/simulation-calendar.md` 的额外月报／预设／延迟范围，以及本记录和 `q23-report-frequency-review.md`。类型优先 ADR 已随 `a6ba588a` 落地，不重复提交。

新增必填 setup 与函数参数还影响既有 fixture／调用方，不能只提交上述生产路径而漏掉编译入口。当前参数接线位于 `packages/engine/src/{diagnostics.rs,verification_evidence/tests.rs,verification_evidence/phase_timing_tests.rs,company/insurance/session_restore_tests.rs,session/decision_chain.rs,session/protocol/civil/publication_tests.rs}`；`packages/engine/examples/{baseline_fixture.rs,simulation_baseline_fixture.rs,escrow_verification_harness/runtime.rs}`；`apps/desktop/src-tauri/src/{actor.rs,lib_tests.rs}` 与 `apps/server/tests/actor.rs`、`apps/server/src/routes/auth_tests.rs`。既有 Engine 测试受影响的是 `tests/{session,auction,civil_clock,company_event_contract,company_query_contract,company_scale,diagnostic_absence,diagnostic_parity,diagnostics,extraction_replay,scale_restore_limits,step_skeleton,tick_frame_commit}.rs`，及 `tests/{attention_discovery/failures/discovery,company_decision_session/main,company_opening/isolation,company_operations/payment_risks,company_operations/session_boundary,company_scenarios/main,publications/prehistory_gold,publications/session_fixture,publications/weekend_publish,save_contract/main}.rs`。工作生成／探针同步为 `current-save-fixture-generator.rs`、`money-wire-golden.rs` 和 `agents/implementation-gap-implementation/parent-checkpoint-expiry-probe.rs` 的 setup 及前史参数；这些入口只需相关 hunk，不能混入其余任务修改。

共享 hunk 的具体约束如下：`prehistory::ensure_original_registered` 现返回真实 sequence，`disclosures` 和 `company_groups` 也采用其余 Q23 更正／税务重述的新 sequence、`ConsolidatedRestated` 和 `corrections::member_adjustments`。若按当前完整文件提交，就必须先具备这套关闭／更正／报告生成依赖，不能只加月报函数签名。`session.rs` 的日终路径和 `persistence.rs` 同时含更正、多人 membership、Q13 时钟／股票开市校验；`candidate_commit.rs` 同时含 Q08 交割回执，月报的可分 hunk 是 mutable events／keys、私有排期公开及二次 key 校验，不包括回执函数。`observation_clock.rs` 依赖当前 Q13 `CivilClock` 语义，不能回退为单交易所时钟来“方便拆分”。Web `App`、`useSaveCommands`、`schema/market`、真实 fixture 同时含多人账户和 IndexedDB；三宿主无需另造月报专用命令，它们通过统一 `SessionSetup` 接收配置。

因此，若不抽取和验证独立 hunk，建议提交的 cohesive 边界是当前“跨层 SessionSetup／Save／公开信息契约与宿主接线”依赖批：月报及其实际引用的 Q23 关闭／重述、Q13 自然日、Q08 回执与多人／日终保存契约一起保持可编译，先完成各模块已登记的独立门禁，再由 root 对最终 index 版本统一构建。该建议不把尚未审查的其它生产逻辑算作月报 PASS，也不要求加入无关经营模型、工具链或未来功能。月报限定静态门禁、host23 六项短绿、root24 集团短绿、root27 普通本人观察短绿、Web23项＋strict save短绿、真实 bindings 导出和 tsc 成功均已有记录；root33 五绿已由 root 和非作者亲核。它们足以支持上述限定签核，尚不能替代最终提交集合的实际 WASM／发布 fixture 验证，更不能外推完整回归通过。

## Web 排期接线与短测证据

- 当前strict `ReportFrequency` 只接受 `Quarterly` 或完整 `Monthly { schedule }`；三层Schedule／Delay变体、1–28日／0–86399秒／1–31天范围及未知字段均严格校验。两布局实际设置控件提供空选择、预设、自定义时分秒及可选随机延迟。裸 `Monthly` 只作为客户端未完draft，不写入Setup／Save；新局在替换屏障前解析并明确拒绝缺排期，不静默补10日。
- 同一已挂载控件通过effect完整回填换档Custom日期／时刻及Preset／delay；先前有效输入变无效时回传未完draft，不能保留旧合法设置继续创建新局。默认Quarterly，UI明确额外游戏月报不替法定报告，年度所得税独立。
- origin `MonthlyDisclosure { schedule, delay_days, seeded }` 严格保存原排期／实抽延迟并按报告period复核真实次月自然日／时刻，拒绝Monthly种类漂移、超配置延迟、错日／错秒及旧 `ScheduledDisclosure.Monthly` 来源。PublicationOrigin既有无ts-rs导出，本地Web结构与Rust源码对照；Frequency／Schedule／Delay／Preset使用实际generated类型，不手写bindings。
- 最初4个Web case实际业务红后转绿：旧parser接受裸Monthly且拒绝完整对象，控件无schedule入口；日志 `.tmp/checklist-wave4/q23-monthly-web-{red,green}.log`。新局裸Monthly实际能够启动的独立业务红转绿为2指定case，`q23-monthly-newgame-{red,green}.log`；origin旧parser拒真实MonthlyDisclosure的业务红转绿在 `q23-monthly-origin-{red,green}.log`。不覆盖Rust初批缺红的历史，不声称整批完整TDD。
- 最后10指定Web短case通过，`q23-monthly-web-boundaries-green.log`：shape2、UI5、新局2、origin1。UI其中2个使用repo既有React dispatcher/effect harness调用真实组件onChange，覆盖preset→delay→custom半填→完整→day29拒→day28合法和同mounted换档；这是hook测试，不是DOM／浏览器E2E。origin含Dec→次月跨年＋31日、Feb28闰年／非闰年跨月延迟、None＋非零延迟及legacy来源拒绝。case及外部进程树期限均10000ms、文件并发4。
- `monthly-bindings-contract.test.ts` 实际读取ts-rs文件并以生成类型消费完整union；旧生成裸Monthly的真实红保留在 `q23-monthly-bindings-red.log`。root `host21` 实际运行96个Engine ts-rs exact导出后，新generated契约单测 **1/1通过**，见 `q23-monthly-bindings-green.log`；随后实际 `tsc --noEmit -p apps/web/tsconfig.app.json` **exit 0**，见 `q23-monthly-types-postexport.log`。没有手写／手补generated或真实JSON。
- 非作者 `review_q08_final` 完整diff及新增交互／日期边界复核PASS，无剩余Web源码阻断；三宿主即时披露／本人获知和真实fixture验收仍由root统筹，不把Web短测当完整跨层端到端。
- 完整本批save commands＋UI＋shape＋binding短批 **23/23通过**，见 `q23-monthly-contract-save-green.log`；strict Save 频率指定case **1/1通过**，见 `q23-monthly-strictsave-green.log`。此前红测原日志保留，不把最终绿外推为真实三宿主披露或完整回归。

用户已确认 `ReportFrequency::Quarterly` 默认，`Monthly` 可选。月报属于额外游戏报告，不替代原有年报、中报及季度报告；公开频率与年度所得税核算相互独立。

当前实施包含必填 `SessionSetup.report_frequency`、三宿主通用输入、Web 两布局的新局设置、draft 与启动/换档回填、严格存档 parser、月报来源契约、前史与日终及集团排期的设置读取。没有旧档补字段或迁移。

用户补充回答：月报公开排期需要同时支持可直接选择的预设方案、在设置中指定的日期／时间，以及可选的随机延迟，例如晚一两天。不能只固定一种月报日期，也不能把可选偏移强加给所有公司；季度默认与法定定期报告保留不变。

报告选择问题的用户回答：同一公司、同一报告期、同一报告范围，NPC 同时已知多种报告时，按“年报＞半年报＞季报＞月报”选择。该顺序只用于本人已经获知的材料，不提前读取尚未获知的年报，不混用 `Standalone` 与 `Consolidated`。同种报告再按版本选择，更正月报不能仅凭更高版本挤掉同期间、同范围已知的年报。实施检查应覆盖四种报告排序、低版本年报优于高版本月报、本人未知年报不参与，以及原有期间／范围选择守卫。

实施 checklist 为：先登记这项回答，再接排期配置与预设选择；显式配置随机延迟，按本局 seed、公司与会计期间一次确定，重试及恢复不重新抽取；前史和实时披露使用同一规则；公布时点必须实际进入公开与本人获知链，不将日内指定时点拖到日终后再伪称已及时公开。具体预设和自定义规则在界面明确展示，用户选择后才用于月报，不用尚未选择的默认日期静默代替。

实施采用必选结构 `ReportFrequency::Monthly { schedule }`，不接受没有排期的旧 `"Monthly"` 字符串。`MonthlyReportSchedule` 可选明确展示的 `FirstDayEvening`（次月1日18:00）、`TenthDayEvening`（次月10日18:00）预设或 `Custom { day, second_of_day, delay }`。自定义日期为次月1–28日，避免对不存在的日期静默钳位；时刻允许当天任何有效秒数。两个预设是供用户主动选择的游戏方案，不是 A 股强制月报规则，也不替用户选择日期。

`MonthlyReportDelay::None` 不添加延迟；`Uniform { max_days }` 显式选择0至指定上限的自然日均匀延迟，上限1–31日。延迟是公司经营 seed、公司身份与会计期间的纯函数，不消耗可变 RNG 状态，重试和恢复不重新抽取。前史与实时公布复用同一函数。公布来源 `MonthlyDisclosure` 保存所选排期、实际延迟和前史标记，恢复时严格复核日期、时刻与报告种类。

交易日月报在私有 tick candidate 中公布，生成真实 `CompanyDisclosurePublished` 事件，完整 commit 成功后才进入权威状态；本人信息通过现有订阅及 cadence 链获取，玩家报表查询使用当前观察时刻而不是当日00:00。当前自然日已完成市场会话后，观察时刻保持15:00，不因市场 `day` 增加而回退至开盘。月报沿用同报告范围的中期材料链，以单月同比修订增长预测、使用本人已知年度基准，不把单月利润直接年化或混用报告范围。休市日以及收盘后的月报在自然日日结处理时按公布时点逐项送达；不制造交易 tick，不将这些时刻冒充日内实时撮合。月度模式的日终派发端点是23:59:59，支持18:00之后的月报；默认季度模式仍保留既有18:00排期及游标。

已结束前史月份通过 `close_month` 真实封账并登记月报；日终公布只读取已封账的月报版本，集团 live 月报检查成员封账。所得税处理仍按年度推进。

TDD 记录：最初未执行红测试便实现接口，此阶段不满足完整红绿过程。用户补充排期后，先新增可执行的 `monthly_frequency_requires_user_selected_schedule_and_optional_delay`，由 root 的 host19 实际短跑取得 `invalid type: map, expected unit` 红证据（`.tmp/checklist-wave4/host19-monthly_frequency_requires_user_selected_schedule_and_optional_delay.log`），再实现带排期结构。非作者复核发现 `Monthly` 误入年度估值路径和收盘后查询时刻回退，两项已修复。host22 的部分失败是新测试夹具误用会计期间代替 DTO 的完整期末日期，以及误取同刻第一次获知的旧报告；已按真实契约和本次真实公布 ID 修正，没有放宽期末、获知时刻或材料使用断言。

host23 对6个月报短 case 实际验证全部通过，总 wall 0.89 秒，使用6个并发进程、每进程 Rayon 8线程、10秒外部 deadline。覆盖必选结构、默认季度四报告保留、可选延迟确定性及非法配置、10:30公开且至最后 tick 仍可读取、真实机构获取新月报并使用该 ID 更新估值、休市日21:00公布及日终恢复后不重复。host22 的独立私有 candidate 故障短测也通过，权威 library、本人认识和业务 hash 保持不变。集团封账及同范围公开的代表性短测已在 host24 通过。

普通本人观察入口遗漏 `Monthly` 分类的 root24／host25 真实红报 `UnknownPublication(0)`，曾被误判为夹具 ID 问题。完整源码和新增查找断言证实该 ID 是存在的 `Monthly` 报告，分类遗漏导致它掉入 `credit_default_cause` 的公告查找；因此这是有效业务红，不更换 ID 或绕过本人获知。仅补 `Monthly` 分类后，原本人报告 ID、获知时刻、估值使用 ID 与因果断言在 root27 实际通过。Web 10 项设置／来源／换档短测及实际生成 bindings 对照也已通过；全量回归未运行。

用户新确认的报告类型优先规则已按独立 TDD 流程落地：旧纯排序测试在 root27 真实失败，最终夹具修为合法 December Annual／Quarter／Monthly 与 June HalfYear／Quarter／Monthly 窗口后，root28 再取得真实合法期间排序红。非 Annual 使用静态一致的 `Correction` 元数据，不借无效 HalfYear December 报告制造失败，也不声称具备这些更正的完整公开历史。生产优先键现为 `(period, consolidated, report_kind_rank, sequence)`，同范围／期间内 `Annual > HalfYear > Quarter > Monthly`，种类优先于该种类内的版本序；原期间／范围优先及只取本人已获知材料不变，机构、散户和公告消费者共用同一 helper。host29 对4项政策短测实际全部通过，wall 0.11 秒、4进程并行、每进程 Rayon 8线程、外部10秒 deadline。非作者独立复核及证据核读单独记录在 `q23-material-priority-review.md`。

继续核查另发现月报重复使用时的年度基准缺口：已经使用某个 `Monthly` ID 后，本人新获知同范围新年报，但最新期间仍是该月报，旧幂等判断因月报 ID 未变化而跳过年度基准更新。真实开账、封月、公开 ID、本人前后获知的短测试在 root30 取得业务红：旧年报来源未换成新本人材料；root31 实际短绿0.01秒，非作者已亲读红绿并通过限定独立门禁。生产现同时检查年度与月报两 ID；月报已用而年度基准变化时仅用新年度事实重估，不重复消费月报增长事实的 λ。完整测试还核验本人未知年报、异 scope 年报、非退化／非固定点预测及前后重复幂等；年度基准永远是 `used_report_ids` 第一项，不凭默认顺序猜测。该项与报告种类排序小提交分开，详见 `q23-monthly-baseline-review.md`。

同窗抽取缺口也已按独立红绿修补：`income.cumulative` 是本年累计，但 `prior_year` 是报告窗口的上年同期，月报2月及三季报原比较口径混用。真实单体／合并四个短夹具使用低于增长 clamp 上限的10%同比，root32 均实际得到旧算法20%对正确10%的业务红；随后复用公开主附注的报告窗口 `movement` checked 汇总，不改分类、预测参数或存档结构。root33 重编后四项确切 case 均通过，增强年度基准 case 同批再次绿；五进程并行、每进程 Rayon 8线程、外部10秒 deadline，整批 wall 0.105秒。非作者分别核读日志与限定源码，年度基准和同比窗口两项 Gate 均 PASS，独立记录见 `q23-monthly-baseline-review.md` 与 `q23-interim-window-review.md`。最终真实 WASM、发布 fixture、存档与月报增量复核仍由 root 统一收口，不把上述部分验证当作整个 Q23 或完整回归完成。
