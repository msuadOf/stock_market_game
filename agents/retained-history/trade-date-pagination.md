# 私有交割单日期分页

本批对应 ADR-0034。保留全部实际 `Fill` 交割事实及原“最近100条／更早成交”入口，新增按日期范围、证券及买卖方向的本人复盘。没有新增盈亏归因、持仓快照、行情读取经历或数据库副作用。

## 契约与边界

- `PersonalTradeHistoryRequest`：`date_from`／`date_to` 两端包含，`code`／`side` 可空，`before_receipt`／`as_of_receipt` 可空，`page_size` 1–100。所有字段均必填，nullable明确写null；无版本、旧字段补齐或默认排期。receipt游标使用完整规范u64十进制字符串，金额继续i64分字符串，股数保留股。
- 返回 owned `PersonalTradeHistoryPage` 包含原请求echo、当页完整交割单、`next_cursor`、排他receipt上界、开局／当前／最后结束自然日。首查询固定当前 `next_receipt_base`，续页传同一上界＋排他before，之后新增Fill不改变既有查询窗口。每页最多100是可继续读取的传输页，不是删除或全范围截断。
- 空私人流水仅证明该范围及filters没有本人成交，不以此伪造休市／量价缺失。日期元数据明确开局前、未发生和尚未完成日终的范围；按证券休市／无交易公共历史由各股历史契约负责，不混入本人交割单权限或Q02行情读取经历。
- 私有查询以只读 `GameSession` 和共享Arc历史为输入，owned只clone当页；不加载／恢复当前市场、不推进tick、不调用Q02 history-read ledger。保留已有全部历史和成功自然日日终保存边界，不触及retained history作者的session history／Save字段。
- 三宿主调用caller不能选择账户。WASM／本地Tauri绑定本地玩家0；Remote经授权subject解析当局membership自己的account，再按generation查询。三Host校验generation／baseline与request echo；Remote还核本人account未切换。
- UI同时提供起止自然日、全部或单股、全部或买卖方向、前／后页、范围刷新、错误详情及空态。前页缓存是已取得的owned查询结果，不重读市场；filters、scope generation／account或宿主变化取消旧承诺，迟到成功／错误不得安装新页。页面只显示真实成交额及各项实际费用，不计算或猜测盈亏归因。

## TDD 与验证状态

- Core DTO／unit先落可编译empty-page红骨架，root统一Cargo的host41真实binary按5线程、外部10000ms执行：3业务红、1只读用例绿，日志 `trade-date-core-red.log`。随后实现只读selector，过滤后仅clone当页并借用一条额外匹配事实判定next；required-nullable及canonicalu64 wire、证券筛选、空账户历史、恰好满页边界已补充，现6unit待fresh binary绿灯，不自行Cargo／生成bindings。fixture仅构造查询事实及一致自然日scope，不宣称完整存档或市场推进fixture。
- Web strict query/page最初透传骨架实际2个业务红，`trade-date-web-red.log`；实现后2/2绿，`trade-date-web-green.log`。覆盖字段完整／日期／account拒绝／cursor／page限制，rows原facts范围、filters、稳定降序receipt、asof上界及scope元数据。
- UI无日期入口骨架实际SSR红，`trade-date-panel-red.log`；真实控件接线后SSR及React dispatcher／effect交互3/3绿，`trade-date-panel-hooks-green.log`。补充真实子组件SSR错误详情与重试后，UI4＋parser2短测6/6绿，`trade-date-web-final-short.log`。前后页100＋1、固定asof、filter及gen/account迟到成功／错误、空态区别及不安装假空页均覆盖；hook／SSR短测不等于浏览器DOM／E2E。
- 三宿主Web小tracecase3/3绿，`trade-date-host-green.log`，检查完整query、无账户参数、Bearer、generation与完整u64游标。新Host方法最初无独立通过编译业务红，不夸称该接线整批完整TDD。
- 每命令外部进程树10000ms，Node case10000ms，文件并发4。首次TypeScript因新Request／Page未导出报缺模块，日志 `trade-date-types-preexport.log`；host42实际3个正规ts-rs export已生成bindings。后续 `tsc -b` 被10000ms deadline终止，`trade-date-types-final.log`，不能称类型检查绿；没有手写generated或真实JSON。所有日志在 `.tmp/retained-history/`。
- host42列举确认6unit＋3export且binary新于源文件，9线程实际执行3export／1wire绿、5fixture红，`trade-date-core-first-fresh.log`。原因是合成查询clock将current移至Jan5却复制session的Jan2 due；fixture已改用独立纯CivilClock的无业务队列scope，保留全部业务断言，不宣称完整Save／市场fixture，等待再次fresh green。
- 非作者复核发现Web需额外约束成交事实属于本局`start_date..current_date`。宽日期范围测试真实红后增加scope guard，开局前／未来拒绝、当前日真实成交接受；parser3＋UI4实际7/7绿，`trade-date-scope-red.log`／`trade-date-scope-green.log`。修复已提交同一非作者复核。

截至host44，root实际Rust编译成功，新Engine binary晚于selector源码且列举实际6项查询unit；6线程、外部10000ms短命令实际6/6绿，`trade-date-core-final-green.log`，0.19秒。既有公开行情／持久化由其他owner负责，本记录不宣称ADR-0034整体完成或复杂回归已执行。

后续root已以完整Cargo环境实际执行109项正规ts-rs export，`host42-web-export-bindings.log`，`DailyCandle.time`恢复当前number契约，不修改generated掩盖问题。私有parser的readonly rows改为owned `[...rows]`匹配实际Vec契约，短测7/7及独立增量复核PASS。root全局TypeScript实际零错误退出0，`history-web-tsc.log`。host43实际Rust编译指出Server私有查询的旧MembershipError closure与SessionError不匹配，已改显式`SendCommandError::Rejected(error.to_string())`保留详情；host44再次实际编译成功，非作者已亲读编译success与Core6绿证据完成本批复核，不将此前静态修复冒充编译绿灯。

## 独立复核

- 非作者 `review_q08_final` 完整审查本批 Core selector／DTO、三宿主本人授权与 generation 接线、Web 严格 parser、日期面板及相关测试，静态复核 PASS。日期范围两端包含、排他 receipt ceiling、owned 当页复制、实际分金额／股数和只读查询契约一致；未发现新增市场推进、行情读取经历或他人账户泄露。
- 初审 P2：合法宽日期请求的回包仅检查 requested range，未约束成交属于本局 `start_date..current_date`，可能安装开局前或未来的伪成交。新增用例实际红后补 scope guard，复核确认开局前／未来拒绝、当前日真实成交接受；亲读 `trade-date-scope-green.log` 的 parser 3＋UI 4，共 7/7 通过，无剩余确定源码问题。
- 合成查询 fixture 改用无业务队列的独立 `CivilClock`，再经既有 `from_parts` 校验，是修复复制旧 disclosure due 导致的前置失败；没有放宽生产 clock／public restore 守卫，也没有删除分页、filters、稳定窗口或只读断言。此 fixture 只证明 selector 的合成事实及日期 scope，不证明完整市场、公共存档或端到端持久化。host42 的前置失败不能充作 selector 业务红；业务红依据仍是先前 empty-page 骨架的真实执行记录。
- 据本批记录，host42 的 3 个实际 ts-rs export 已通过并生成 bindings；修正 fixture 后的 6 个 Core unit 仍待 fresh binary 验证，TypeScript 命令曾被十秒 deadline 终止，尚不能称类型检查通过。独立复核者未运行 Cargo、测试、生成绑定或修改源码；静态 PASS 不代表 ADR-0034 整体、复杂回归或浏览器 DOM／E2E 验收完成。
- 后续 fresh TypeScript 检查发现 normalized rows 的 readonly 数组不适配真实 generated `Vec` 的 mutable 数组。唯一修复为返回 `confirmations: [...rows]`：只复制最多当页 100 个已验证记录引用，取得 owned 数组，不改变金额、排序或事实，不使用 cast，也不修改 generated。非作者增量静态复核 PASS，亲读重跑 `trade-date-scope-green.log` 的 7/7 绿色结果；此局部类型修复不代替完整 TypeScript 检查通过的实际证据。
- host43 实际编译指出 Server `MemberRequest::TradeHistory` 复用的 `reject` closure 已被推断为 `MembershipError`，不能接收查询返回的 `SessionError`。修复仅将该处改为显式 `SendCommandError::Rejected(error.to_string())`，与相邻 `History` 接线一致，完整保留错误详情、本人账户解析及 generation 守卫；非作者最小增量静态复核 PASS。主协调者报告 `history-web-tsc.log` 的全局 TypeScript 检查零错误、exit 0；fresh Rust 重编译与 Core 绿色结果仍待确认，不将编译失败称为业务红。
- 最终证据收口：非作者亲读 host44 的 `host44-history-source-red-build.jsonl`，末项 `build-finished.success=true`，Engine test artifact 为 `fresh=false` 的 `engine-48220fcc6e1a5073`，实际 binary 时间晚于 selector 源码；亲读 `trade-date-core-final-green.log`，六个具名 Core unit 全部执行，6 passed／0 failed，耗时 0.19 秒，不是 zero-test 成功。主协调者说明执行采用六线程和外部 10000ms deadline；复核者没有自行构建或重跑。
- TypeScript 日志 `.tmp/checklist-wave4/history-web-tsc.log` 已亲读，为零字节、无诊断；exit 0 来自主协调者实际执行报告，不从空日志单独推导退出码。结合已核对的实际 bindings 与 Web 7/7 短测、本批完整 diff 及修复，私有本人交割单日期分页本批独立复核最终 PASS。此前各阶段待验证表述保留为历史过程，不再作为当前未完成结论；该 PASS 不覆盖 ADR-0034 公共量价、数据库持久化、完整经济运行或浏览器端到端验收。
