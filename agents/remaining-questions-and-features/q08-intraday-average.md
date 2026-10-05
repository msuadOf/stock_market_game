# Q08 分时均价实施记录

## 当前边界

- 移动分时均价按当前交易日真实成交额（分）除以真实成交股数（股），即 `Σ(price × qty) / Σqty`。分子来自引擎 `DailyTradeStats.turnover_cents`，分母来自同一活动日 K 的 `volume`；两项必须取自同一个 `NormalizedTickFrame.activeDailyCandles[code]`。
- `MarketChartProjection` 将上述十进制成交额字符串和累计股数附在分时点上。均价线使用各点的权威日累计分子/分母；窗口裁剪不从可见价格点重新求均值。baseline/重连立即显示 snapshot 中活动日 K 的累计均价；缺少逐时点历史时只显示现有可证点，不重建不存在的分钟线。
- 每交易日行情重建清空前日点；日累计统计由当前 snapshot/frame 接续。日内均价不从最近逐笔缓存累计，不用竞价指示价、昨收或分钟末价补造。
- `packages/engine/src/intraday_average.rs` 提供纯 Rust `calculate_intraday_average`：读取现有 `DailyTradeStats` 和 `volume_shares`，保留精确成交额分子、股数分母，不提前舍入；缺统计或一致零成交返回 `None`，成交额/股数/笔数不一致返回显式错误。
- `UX-CONTRACT.md` 登记口径、单位、缺失状态、数据源选择和交割单边界。当前已有默认前端／显式Rust设置、三宿主capability与按需batch、真实Fill历史保存恢复和本人分页UI；不把交割单当公开逐笔事件，不新增逐笔行情存档或实时GPU。

## TDD 与验证

- 当前成员接线后重新执行：Web代表性25/25、三宿主Q08指定pattern4/4、实际TypeScript检查exit0，日志分别为 `.tmp/checklist-wave4/q08-current-web-green.log`／`q08-current-host-web-green.log`／`q08-current-types-green.log`。命令与Node case均10000ms、并发4；这是当前短测及类型证据，不是完整回归或新成员非空流水端到端证明。

- Web 先改测试运行红灯：旧投影把 `10` 与 `8` 算术平均为 `9`，空成交时使用昨收/竞价价显示；新测试分别命中真实累计成交额/股数、零成交、缺失统计和可见窗口截断。
- 实现后定向 Web 测试：`mobile-intraday-projection.test.ts`、`market-chart-projection.test.ts`、`mobile-component-render.test.ts`，29/29 通过；由 `run-with-deadline.mjs 10000` 外部进程树监督，各 Node case timeout 10000ms。
- Rust 新增三个单测：`keeps_exact_turnover_and_share_ratio_without_rounding`、`missing_statistics_and_consistent_zero_trades_are_unavailable`、`inconsistent_statistics_fail_explicitly`。root 统一构建日志 `.engine-build-2.jsonl` 记录成功编译；三个 exact case 分别 0.00s，通过独立 10000ms 外部期限并行执行；两次 binary 验证均通过。没有运行完整 Rust 回归。

## 最初待办与当前接线

- 玩家source preference默认前端、可选Rust已接两布局，刷新重置；三宿主使用相同能力和按需语义。
- `HostCapabilities` 按指标明确Rust支持，选择capability=false显示“不支持”、不退回前端；三宿主现有纯batch入口读取客户端已收到的权威facts，不冒称Server重新查询历史。
- 必需交割单现从成功CommitTick的实际双方Fill及charged构造，共享历史进入必填runtime，按本人分页展示；完整回归、非空跨subject三宿主端到端矩阵和Browser真实持久producer验收仍未从本项短测证明。

## 本轮新增实现

- Engine 暴露 `GameSession::query_intraday_average`，只从同一只证券的活动日K读取真实 `trade_stats` 与累计 `volume`，未知证券和统计不一致均显式失败；跨宿主曲线入口是纯batch计算，不把任意提交样本认证为权威行情。
- 新增 `PersonalTradeConfirmation` DTO 与按账户查询。Tick candidate 在 CommitTick 前从真实 `Fill` receipts 构造确认：receipt index、自然日、证券、方向、逐笔数量/成交额/成交价及该 receipt 的实际收费；不读取公开 `Trade` events。候选失败或尚未提交时，权威会话查询不受影响。
- 确认记录进入可提交状态并纳入 `SavedRuntimeState` 必填字段；恢复时严格校验账户、receipt 顺序/范围、自然日、证券、正成交量/成交额、价格乘数量以及非负实际费用。没有旧存档默认字段或迁移。
- 低层 `GameSession::save/restore` 可用于内存验证，禁止的是日内对外保存／写文件。公开保存边界和真实Protocol日终完整双方历史已由host-build-11代表性短测验证；Browser真实文件／IndexedDB fixture正在改为Protocol日终producer，未从低层内存投影冒称公共持久档。
- TDD证据限制保留：最初新增真实成交测试因缺query API产生编译红，后续DTO被FeeComponents缺Deserialize阻断，不能把这段称为先业务红。后续真实运行红→修→绿及host-build-11结果见后节，不用最终绿伪造早期TDD过程。

## 本轮增量接线

- `EngineHost` 增加单请求 `calculateIntradayAverageCurve`，样本上限 600；Worker/WASM、Remote Server 与 Tauri 均回传同一 `seriesKey` 和有序结果数组。Remote Server 在单个认证请求中转发整批样本，并逐项校验同一会话 generation；Tauri 在本地 timeline 查询游标下校验 generation；Worker 在请求关联和 baseline epoch 下校验；客户端丢弃过期响应，不把跨证券或跨游戏日结果安装到当前图表。
- Desktop `PriceChart` 与移动详情共用 Redux 的 source preference 与 `resolveIndicatorRoute`。前端源使用每个真实累计成交额/股数点；Rust 源一次批量传送完整事实曲线，再按相同时间槽绘线；选中 source 不支持时显示显式 unsupported，不回退。图表实时输入仍须有真实成交统计，不使用价量近似伪造。
- 移动详情与桌面复用 `TradeConfirmationTable` 的本人按需入口，不从公开Trade补造。查询账户由宿主本人上下文确定：Worker／本地Tauri的单玩家仍为0；Remote现从认证subject解析其当前membership.account_id，不再把所有远程玩家当0，不接受caller传账户。
- 历史Web定向短测计数保留：market chart projection＋indicator source11/11、mobile13/13、Worker30/30、Tauri18/18、Remote18/18、curve2/2、chart runtime6/6，外部／case均10000ms。该时点的report_frequency／ExchangeClosed旧生成类型错误已由root后续真实ts-rs导出解除，当前TypeScript已实测绿；这些旧计数不等于新增membership路径已重新完整验收。
- 本增量完成后的root统一构建、Protocol日终／日内保存拒绝及非作者复核结果见后节。最初曲线整条实现没有先可编译业务红证据的限制不撤销；不把后来边界红绿短测说成整条曲线完整TDD。

## 最终接管增量（2026-10-05）

- 补齐此前遗漏的 Web `SavedRuntimeState` 必填 `personal_trade_confirmations`：严格字段、规范完整 u64 receipt、i64 分金额、u32 正股数、价格乘数量与成交额、非负实收费用、全局 receipt 唯一／账户内递增／小于游标，以及账户、证券、自然日的跨层关联校验。缺字段或旧结构直接拒绝，没有兼容默认值。
- 公共本人查询是newest-first最多100条receipt范围页，账户绑定宿主本人：本地0、Remote认证subject对应账户。`beforeReceipt`是完整u64字符串，三宿主均不接caller自选账户，移动／桌面有更早成交入口。Arc chunk CoW只复制有界尾块，checkpoint共享旧历史，保存时才完整投影。
- Remote 曲线改为单一 actor 命令调用纯 Rust batch，而不是逐 sample 往返。单点／曲线 POST generation 采用当前十进制 string 契约；Web 查询结果逐项与原始成交 facts 核对。backend 输入来自客户端已收到的权威 facts，不是 Server authority 重读历史，不把调用者自行构造的数据冒称权威行情。
- 新增真实Protocol测试覆盖同一卖委托两次部分Fill的佣金增量、成功自然日日终完整双方历史、恢复后续receipt、日内公共save仅先前日终候选；malformed tick／civil回滚不泄漏，分页与checkpoint共享旧块。该接管阶段尚未运行，后续root统一host-build-11真实10/10结果见后节，不把历史待运行语句继续当当前状态。
- Web 新增完整金额／ID边界、必填历史存档、畸形 fulfilled curve 回包、移动 KDJ／MA source 无 fallback 的可执行红→绿证据。此前曲线整批实现缺少业务红证据的限制仍保留，不把接管补测说成整批完整 TDD。
- 独立 reviewer `/root/finish_q08_delivery/review_q08_final` 初审发现 Remote POST generation 类型冲突、移动 fulfilled throw 未 catch、移动 KDJ 无视所选源、已安装交割单跨 generation 残留；全部修复后复核通过。进一步 MA 漂移也按 `rust` 显式 unsupported 处理；去掉 PriceChart 重复 unsupported 提示。复核覆盖 A 股语义、必要范围、回滚／恢复／游标、授权、编码、CoW 与 stale 查询，不替代 root 构建结果。
- 接管时Web21/21、三host Q08 pattern4/4、source policy＋Redux3/3通过，各批命令／case10000ms并发4。该时点全host65/66的旧sourcefixture缺runtime必填字段错误保留为历史证据；后续sourcefixture按当前契约更新，不建立旧档fallback。真实公共Save fixture须由root Protocol producer生成，不能用合成fixture或低层GameSession投影宣称有效；当前类型检查已真实绿。

## 真实 Rust 收口验证

- root `host-build-8` 首次真实定向执行为 6 绿 4 红，不隐去失败：额外添加的第二 `Player` 不属于 `setup`，原 Protocol 恢复 fixture 被合法账户集合校验拒绝；lower fixture 日期落在现已建模的元旦休市日；Server 测试开局 source helper 缺当前必填 `report_frequency`。修正 fixture，不放宽账户、日历或存档校验，不手补生成 JSON。
- 合法 Protocol fixture 采用工厂已有的 `Inst` NPC 与玩家。原有 600 股先真实初始化给 NPC，再用覆盖式 `grant_position` 转配成 NPC 400 股＋玩家 200 股，断言总量不变、NPC cash 不变。卖的是玩家开局股份，不是当天买入 T+1 锁定股份；NPC 预置 bid 999／ask 1000 不相交，不以自成交承担验收。关注调度只在短 fixture 隔离到 tick 1000，策略元数据保留。
- root要求全部真实双方内部事实，已移除strategy账户过滤：所有账户实际Fill进入committable history。host-build-11时公共三宿主绑定0，随后Remote公共路径改认证subject self，本地仍合法单玩家0。Protocol断言本人和NPC完整4→6链／实际费用／日终保存恢复／续游标，不把NPC内部历史公开给caller。
- `host-build-10` 实际 7 绿 3 红：fixture 每添加一个 quote 就 hydrate，第二单触发现有 live ledger 与订单集合不一致校验。改为三个初始 quote 完整安装后一次 hydrate，恢复后各次空簿安装再 hydrate；没有重置或放宽权威 ledger。
- `host-build-11` 从实际 Cargo JSONL 解析 engine／server／desktop／WASM 四个 test 产物，逐个真实 `--list` 后执行 10 个 Q08 exact case：平均线 4、Protocol 3、真实 Fill 2、Server 真授权 query／curve／generation parser 1，**10/10 通过**。四进程并发、`RAYON_NUM_THREADS=8`、`--test-threads=8`，每命令外部 10000ms deadline；单 case 实测 0.01–0.71 秒。runner 为 `q08-short-validation.mjs`，原清单、policy、结果及日志在 `.tmp/checklist-wave4/q08-short-validation/host-build-11/`；build-8／build-10 红证据保留，未覆盖。
- 非作者 `review_q08_final` 复核 fixture 必要性、股份／现金守恒、T+1、全部账户内部历史和公共权限隔离后通过；确认 host-build-11 的十个结果 `error=null`，没有把复核当成自行运行 Cargo。上述短测不代表完整回归或三宿主非空交割单端到端平台矩阵。

## 成员接线后的当前核对

- Remote公开路径实际为 `authorized_session` 返回认证subject，actor `confirmations_for` 在同一generation上执行 `resolve_trading_account(subject)` 后读取该账户receipt范围页。旧内部固定0 command不再是公开HTTP入口；Worker／本地Tauri仍是合法本地单玩家0，不能把三宿主都描述为已经具备远程subject登录。三宿主caller都无自选account字段。
- 两个真实UI wrapper已按 `generation:selectPlayerAccountId` 重挂交割单子树，处理换档及同市场换membership账户，不只依赖generation。内部NPC双方history保留，公开只交付宿主本人；未入场Remote身份不读取0账户历史。
- root确认最新97个实际ts-rs导出完成；本项再次实际 `tsc --noEmit -p apps/web/tsconfig.app.json` **exit 0**，日志 `.tmp/checklist-wave4/q08-current-types-green.log`。当前25个Web短case **25/25**，`q08-current-web-green.log`；三宿主Q08指定pattern **4/4**，`q08-current-host-web-green.log`，命令／case10000ms，文件并发4。没有自行Cargo、导出或手写generated。
- host-build-11的十个真实Rustexact绿色证据仍只证明当时的代表性业务边界。成员／subject接线另依本轮membership审查，不把当前mock宿主空交割单4case冒称非空跨subject私有流水真实隔离验收。root正在将 `refresh_real_save_fixture` producer 从低层GameSession投影改成Protocol成功自然日日终候选；Browser真实恢复／持久化验证待该批冻结，不用合成sourcefixture或日内checkpoint默认补事实替代公共EOD证明。
