# sweep62：frontend OOP 后续 review／verification／WASM 全文核对

## 范围和读取

- 按主控指定 `b76ece3` merge同产品源码审查，目录 `.worktree/implementation-reaudit`。
- `agents/oop-refactor-implementation/frontend/review.md` 230行，连续读完；初次输出155–179被截断，已用独立段完整补读。
- `agents/oop-refactor-implementation/frontend/verification.md` 36行，连续读完。
- `agents/oop-refactor-implementation/frontend/wasm.md` 77行，连续读完。合计343行。
- 正式ADR-0010／ADR-0025与工程原则优先；OOP工作记录的“保持既有缺陷”不能证明旧产品合同已兑现，也不自动把另批行为修复变成本轮提取需求。
- 只新增本记录，未运行测试、编译、长验收，未修改产品或Git。文档内owner历史运行与reviewer只读结论保持分离。

## review.md 逐章条款矩阵

| 原文章节／行 | 状态 | 当前代码与反证 |
|---|---|---|
| 范围3–7 | 历史基线／review边界 | 本轮不用b89旧hash冒称当前70文件身份；全文包含未跟踪文件是审计要求，不是产品功能。 |
| R2-N01/N04首轮9–27 | 请求身份／registry提取已接 | `components/useIndicatorResults.ts:27`capture request，`:31`／`:35`成功/失败经同gate；`indicator-results.ts:69`／`:79`拒旧结果，`:72`按prices和可选candles长度校验。`host/company-request-registry.ts:5`递增ticket，`:16`只结束当前ticket，`:20`clear不重置sequence；协调器消费同registry，未新增第二authority。 |
| 后续范围29–31 | 已由后文最终review覆盖 | 不将“其余owner在途”当当前未完成判据。 |
| mobile/prototype33–45 | 提取已实现；既有UX局部G仍在 | `design/ui/mobile/mobile-trading-concept.html:147`bindEvents只绑图表/导航/progress，`:154`／`:160`／`:174`实际方法。没有header返回/quick订单旧caller，不以候选原文虚构遗漏。统一tradeTime、量柱方向、逐笔顺序等为既有G10–G14，纯移动APPROVE不能核销正式UX合同；SSR/VM不是浏览器像素证据。 |
| remote47–61 | 窄owner主干已接，明示残余须另核产品合同 | `remote-host.ts:46`publisher、`:47`reports、`:48`commands；`:55`fail拒当前waiter与commands；`:80`先install后callback/waiter；`remote-command-registry.ts:14`只确认入队，`:28`真实fatal批量拒。旧onerror／单槽waiter／dispose pending残余见下节。 |
| WASM63–74 | slot／loop生产owner已接；没有伪称全失败原子 | `wasm-worker.ts:141`create委托slot，`:227`restore，`:233`microtask restart，`:239`nextGeneration与`:242`baseline。`wasm-session-slot.ts:18`唯一handle/generation；`wasm-tick-loop.ts:24`timer/节拍/速度owner，`:66`protocol先发再barrier。异常binding反证见下节。 |
| fixture具名修正76–78 | 测试静态规则修正 | 不影响生产指标公式或gate，不登记新功能。 |
| N06/R2-N03 80–94 | WorkerScope／TauriTimeline提取已有 | `worker-request.ts:26`逐host请求sequence/resources，`:54`／`:55`双匹配，`:64`10s超时清理；`tauri-timeline-state.ts:78`恢复严格下一generation、`:91`parse snapshot。Worker nextGeneration单调性另已sweep29 C29-01，Tauri真实guard不能核销Worker边界。 |
| FR01原始／A27N02N03首轮96–121 | FR01被后续修正关闭，cache/grid/runtime存在 | `components/market-grid-row-synchronizer.ts:27`新grid初始rows后diff提交，`:33`只解除API；market projection owner存在。正式G31未变化股票引用刷新仍是旧局部问题，不能因stable getter名字核销。 |
| FR01修正123–130 | **已修** | `price-chart-runtime.ts:218`每个remove成功立即`:223`null字段；后续remove抛错仍保留准确部分状态。不是旧批量清字段故障。半初始化rollback未获本提取范围承诺，见下节。 |
| App132–153 | 生命周期／save／trade／polling各owner生产已接 | `useSaveCommands.ts:107`／`:173`／`:221`换局前invalidate、idle；`useSessionHostLifecycle.ts:90`单读档、`:105`推进前restore；`useTradingCommands.ts:36`查询gate+host identity；`useSpeedMetricsPolling.ts:21`旧请求gate、`:27`显式错误、`:29`1s重试。日内save只提示/选择目标，真实候选保存仍在App CivilUpdate composition root。 |
| 类型增量155–169 | fixture typing与窄端口修正 | 工作记录没有要求新增DOM模拟能力，不能把React私有dispatcher／SSR／fake chart/IPC当真实浏览器矩阵。 |
| 最终三门171–178 | 独立源码review结论绑定历史manifest | 不重算或伪造70文件SHA，不复述执行gate为本轮通过。概念与交易语义保持不等于已修旧UX/host缺陷。 |
| FR02首轮180–184 | 台账原caller错已由后文关闭 | production caller与test/related owner分离，不把callee当入口，不新增原型header返回/quick订单绑定。 |
| TradingCommands186–195 | 最后依赖修正已接 | `useTradingCommands.ts:51`deps为setNotice与两个稳定ref对象，不依赖.current／表单／generation；旧callback身份契约保持。 |
| FR02最终197–230 | 39需求／16动作／3扩展台账已核销，范围准确 | 224行明确N06 dispose/fatal立即reject与同步throw立即清理为另批行为修复；不把这一OOP scope_exclusion单独升级已确认产品G。70file manifest与最后lint/tsc数字均属历史执行证据，本轮未跑。 |

## verification.md 逐章状态

| 行／章节 | 当前判断 |
|---|---|
| 1–5：实现／review状态 | 后续完整review优先于早期“待review”；FR01/FR02均有修正证据，不重开历史阻塞。 |
| TypeScript7–19 | Node/app/inspector三个project当批并行deadline证据；首轮失败、fixture泛型最小修正与最后全绿保持不同版本。此前work-status旧inspector TS2769不再可作为“现行永远失败”，但本轮不执行tsc。 |
| lint21–32 | 最后69文件零warning是当批精确lint，不等于CI配置warning-as-error已接。根web lint仍裸oxlint，旧G26保持。ref deps与身份测试有当前代码，11case/6.37s不是本轮结果。 |
| FR02说明34 | 原型真实绑定见HTML:147，台账分类修正已落实；不能把未接header/quick按钮当OOP漏提取，正式产品UX需单独比对。 |
| 范围限制36 | 无完整回归／build／E2E／真实三宿主；普通精确case与deadline是既有runner合同。dispose/半初始化残余明确未修，review通过不是这些边界的修复证明。 |

## wasm.md 逐章状态

| 原文行／条款 | 状态与当前caller |
|---|---|
| 范围3–11 | N04/N05 owner提取范围准确，未改schema/交易规则；已有helper生产沿用。 |
| Owner/caller13–25 | `wasm-session-slot.ts:31`generation守卫、`:38`handle、`:44`create、`:53`restore、`:70`prepare、`:75`drop；`wasm-tick-loop.ts:47`speed、`:54`frame rate、`:59`pause、`:76`step、`:89`frame、`:108`start、`:116`stop。Worker真实message case委托同owner，不另持第二handle/timer。 |
| 顺序／既有缺陷27–36 | `wasm-restore-transaction.ts:18`restore candidate、`:19`snapshot、`:21`换handle、`:23`drop旧、`:25`finally cleanup/restart；`wasm-session-slot.ts:64`drop成功后generation推进、`:66`prepare。错误部分authority确实保留，但真实drop binding反证见下节。 |
| tick与publish37–40 | `wasm-tick-loop.ts:92`Infinity每task一步；`:95`有限倍率单步追赶；`:68`protocol先交付、`:69`barrier后stop；CivilUpdate不计市场tick。固定高倍率缺聚合旧G18/G19另在，不因loop owner存在核销。 |
| save TDZ41–42 | 已修：`wasm-worker.ts:211`局部savedSlot，不遮蔽slot owner，wire仍slot字段。 |
| 验证44–69 | 历史57case、listener fixture、fake WASM/port/timer及源码搜索；不是本轮build、真实Worker/WASM或三宿主E2E。缺module首红仅证明owner API不存在，不伪称旧业务失败。 |
| 语义／未完成71–77 | 后续统一review已完成该批门禁，早期“待review”过时；A股和日终保存语义不新增制度。 |

## 候选 C62-01：旧 Remote socket onerror 可以关闭新连接

来源：`frontend/review.md:49`明示“旧 onerror……既有缺陷保持”。当前`apps/web/src/host/remote-host.ts:129`旧socket.onerror直接调用全局fail，不核对connection identity／disposed。同文件`:204`delivery切换递增identity并换socket；`:76`message和`:132`close有identity guard，onerror没有。

实际caller影响：用户切换push/pull后新socket已创建，旧socket迟到error仍执行fail；`:57`detach的是**当前新socket**，`:60`close新连接，`:61`拒当前commands，`:62`交付fatal。这不是G03 pull轮询缺失或G05当前socket失活重连的同一遗漏。

生产反证：当前socket发生真实error应显式fatal，不能为了隔离旧socket吞当前错误；保护应只隔离已废弃连接。本轮未运行，但已有`remote-state-contract.test.ts:107–116`精确测试明确断言旧onerror关闭新socket并报REMOTE_SOCKET，说明提取有意保留而非本轮新引入。

正式合同关联为ADR-0010统一宿主状态/错误边界（`:12`换宿主不改变更新错误语义、`:56`基线限定用途）与工程最小惊讶；这些未逐字规定WebSocket connection identity，**故交主控按功能缺口候选裁定，不仅凭OOP备注升级正式G**。旧总账主表及reaudit-host未检索到旧onerror这一具体项；主控应和其他sweep去重。明确“旧缺陷保持”只限定本提取范围，不表示产品修复永远不需要。

## 其余残余的反证与边界

- Remote单槽resync waiter覆盖：`remote-publisher-state.ts:34`确实直接替换，`remote-state-contract.test.ts:119`旧Promise不settle。dispose command未settle：`remote-host.ts:168`不调用commands.rejectAll，`:145`测试登记该边界。这些是可见代码事实，但需要正式并发resync／销毁取消合同或实际App死等路径再确认为生产漏实现；不借OOP范围添加timeout/新retry/配额。Worker原请求dispose只待10s属于已有reaudit-host生命周期债。
- Remote fail保留cache：`remote-publisher-state.ts:17`旧baseline仍可读取，fail仅detach socket。旧G04缓存不随delta推进已记录；不能重复将每个getter另算G。C62-01单列是旧connection error作用到新socket，不是缓存泛称。
- WASM重复create覆盖旧handle：`wasm-session-slot.ts:48`注释及源码属内部边界；正常createWorkerHost每次新Worker，未定位正常新局同Worker重复create入口，sweep40已有同反证，不新建用户资源泄漏G。
- 旧drop失败部分authority／candidate cleanup遮蔽原错：helper静态路径存在且`wasm-session-slot.test.ts:76`／`:115`注入fake binding验证保留。但真实`apps/web-wasm/src/lib.rs:475`drop_session只registry.remove，返回void，没有生产Result错误出口；不能把fake drop throw当正常真实Rust返回错误。prepare_public_baseline在同文件`:360`对有效handle委托内存准备，尚未定位真实有效会话下该异常路径。panic／失效binding故障不能冒称可恢复普通日终加载失败。
- Tauri malformed snapshot前写timeline/generation：`tauri-timeline-state.ts:78`guard真实下一generation，`:81`／`:82`写后`:83`parse；工作记录承认部分状态。不直接假设受信RustDTO会输出畸形快照，保留异常协议测试/错误路径复核方向；Worker C29-01异常nextGeneration守卫遗漏是不同具体项。
- chart半初始化rollback与dispose部分remove失败：FR01逐指标成功清ref已修；其他资源初始化/销毁需结合真实图表API失败和正式UX错误反馈再判，不因为记录说“未修”就扩成所有Chart生命周期承诺。

本批新增一个待主控裁定的具体候选C62-01；其余需求已实现、旧G仍保留或是明确另批的异常边界。没有把OOP独立APPROVE、hash签名或历史短测结果当完整产品缺陷清零证明。
