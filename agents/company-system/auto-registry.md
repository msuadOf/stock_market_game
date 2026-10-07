# N2a 批：开局自动名册、面值法定事实、后入股东税账与温和默认偏好（2026-10-08）

## 范围与依据

- 产品决策：2026-10-08 用户决策「开局自动建名册（推荐）——新局创建时自动为每家
  公司按初始筹码分配结果建完整股东名册+法定事实（面值默认 1 元/股、新局可编辑，
  注册资本=面值×总股本），开局即可触发分红/送转等；后续新进股东入册时同步补开
  个人税账」+「新局温和默认偏好——每家公司自带一套温和默认偏好（盈利达标+可
  分配利润达阈值就分红），参数新局可编辑/可关」（ADR-0037 追记）。coordinator
  自定项：全流通名册（无发行人自持股——Simple 无发行人真实账户，自持股语义留给
  Simulation 分支，已在 trading-rules 登记）。
- 实现落点：`packages/engine/src/session.rs`（SessionSetup 两个新严格持久化字段
  `par_value_per_share` / `auto_corporate_foundation` + 校验 + `GameSession::new`
  装配调用 + 私有 `assemble_corporate_foundation`）；`session/corporate_actions.rs`
  （`close_registries_through` 后入股东维护钩子 + `register_late_holder_tax_book`）；
  Web `DEFAULT_SETUP`（温和默认偏好 + 面值/开关默认值）、`parseSetup`（两字段同构
  严格解析）、新局 `ParValueInput` 与 useSaveCommands/App 接线。全部复用既有状态机
  入口（`configure_share_registry` / `define_dividend_legal_facts` /
  `configure_cash_dividend_tax_book` 既有自动开账路径），不复制任何校验。

## 关键口径（详见 docs/trading-rules.md「开局自动名册、面值法定事实与温和默认偏好」节）

- 名册结算日取开局日**前一自然日**：`close_registries_through` 只推进
  (settled_on, through] 区间，开局日自身的成交也必须进入名册日结（红→绿发现的
  真实缺陷：首个 fixture producer 运行暴露「日终账户实际持仓与名册不一致」，
  若 settled_on=开局日则首日成交永远丢失）。
- 未分配余量（总股本−流通盘）登记为单一具名外部股东 `session-auto:founding:{code}`
  （InitialAllocation 来源、无游戏账户、不参与交易、不代扣、分红走外部回执）。
- 后入股东钩子：首次经二级市场净买入入册的账户（`close_registries_through` 的
  日净额残差，结构保证此前不在名册）在 `AShareIndividual` 模式且「个人」身份时
  自动开 `IndividualPublicMarket` 税账；开账日=入册日前一自然日、空 opening lots，
  入册日取得事实由既有 `sync_dividend_tax_days` 作首条日结入账；入册前分红不追溯
  （结构边界）。Flat/Exempt 不开账（Flat 付款日代扣不变）。
- 温和默认偏好数值：3000bp／100000000 分／1 周期（与 CompanyPreferencesInput
  勾选预设同值），送转 null；只在 Web DEFAULT_SETUP 层提供，引擎无默认。
- `auto_corporate_foundation=false` 保持既有显式装配语义（供既有手工装配测试与
  escrow 验证 harness 使用）；产品默认 true。

## 红绿证据（日志在 `.tmp/company-system/auto-registry/`，命令均带
10000ms 外部 deadline；编译预热 `--no-run` 单独执行）

- 红（行为红）：`red-disabled-assembly.log`——临时禁用装配调用后 9 项依赖装配的
  新用例全部行为红（名册/法定事实/税账/偏好闭环/后入股东/恢复深等），3 项不依赖
  装配的持久化用例照常通过。
- 绿：`green-assembly-enabled.log`（新模块 `session::auto_registry_session_tests`
  12/12）；`group-*.log` 受影响组全绿：simple-prefs 16、tax-mode 20、
  simple-session 36、corporate-actions 15、issuer-repurchase 5、rights-offering 13、
  share-split 8、probe 7、company::simple 119、share-registry 28、auto-registry 12；
  `group-persistence.log` 31 过 13 败——13 败与 handoff 登记的既有基线一致
  （saved_runtime 组日历/订单簿环境性失败，与本批无关）。
- 完整 `engine --lib`（300000ms 长验收预算、16 线程）：
  `engine-lib-full-2.log` 1603 passed / 257 failed——失败数与 main 既有基线 257
  完全一致（pipeline transaction 等既有族），本批 +39 通过（12 项新用例＋组内
  计入）、零新增失败。过程中一次 258 失败为本批测试自身过时断言
  （settled_on 改前一日后未同步），修复后复跑回到 257。
- fixtures：三 producer（rustc 直连 release engine rlib）+ company 切片由本树
  重生成（`rebuild-producers.sh`），producer 内置守卫全过：名册恰好 N 只、守恒、
  具名外部股东、无 IssuerTreasury、注册资本=面值×总股本（元两位小数换算回分
  比对）、温和默认偏好值、restore/resave 深等。
- **市场侧轨迹归因（如实登记）**：新主档与已装主档的市场侧（belief_books/
  snapshot/分钟史等）存在差异；控制实验证明该差异**非本批引入**——同一二进制
  同参数连跑两次即产生同量级差异（`fo-run2`/`base-run2`：next_order_id
  457→444 / 463→497），属 ADR-0017 并发受理调度在多进程下的既有非确定性；
  逐 leaf 计数：同二进制两次差异 404 vs 关开关对照 447，同量级。fixtures 承载
  一条合法轨迹，结构性差异（新字段/名册/法定事实/偏好）才是本批预期变化。
- Web：新 `par-value-foundation.test.ts` 5/5（parseSetup 正负例 + DEFAULT_SETUP
  默认值 + 温和偏好同值断言）；机制开关/三层税/价格笼/公司契约/财报频率 22/22；
  save-commands/company-config-commands/company-system-config/company-preferences-input
  35/35；fixtures 消费者 14 文件全绿（其中 3 个改写账户集合的既有用例按 N2a 名册
  勾稽显式移除名册后断言，测试意图不变：market-memberships 10/10、account-id-contract
  10/10、save-schema-contract 含 Money 用例全绿）；tsc -b --force 0 错误；oxlint
  本批文件 0 警告；typegen 正规重生成（SessionSetup 新字段），check 脚本红仅因
  未提交（流程预期）。Web 全量 8 项失败均为既有/环境性（initial-allocation-transport
  Worker mock、wasm-worker-ownership、runtime-strategy-position 的 schema fixture），
  与本批改动面无关。
- `cargo check --workspace --all-targets --exclude stock-market-game` 通过；
  `cargo check -p web-wasm` 通过。

## 独立复核修复轮（2026-10-08，fix-round）

非作者 subagent 审完整 diff（dc311164..09fceaae，8 angles）：无 major、
minor 3、note 3。三项 minor 均已修复并复测：

1. **缺「入册日 == 股权登记日」边界用例**（minor-1）：新增
   `late_holder_buying_on_registration_day_is_included_in_snapshot_same_day`
   ——测试侧独立推导登记日（逐自然日查日历），登记日当日真实买入入册，
   断言纳入当日登记快照（登记日收盘持有即享有）、税账取得日=登记日、
   `registered_on`==登记日（持有期自该日起算）且付款税前全额到账；模块
   13/13 绿。复核确认代码顺序本就正确（名册推进先于快照冻结），用例锁
   定该组合不回归。
2. **ParValueInput 逐击键提交半截值**（minor-2）：改为**失焦（或回车）
   提交合法值**，非法文本永不写草稿并提示「未提交的编辑：失焦后写入」；
   新增 `ParValueInput.test.ts`（SSR 渲染默认草稿/失焦提示＋元↔分换算
   正负例）2/2 绿。
3. **auto 开关跨局静默粘滞**（minor-3）：`useSaveCommands` 新局组装显式
   `auto_corporate_foundation: true`（新局创建界面无关闭入口，统一回产品
   默认开；读档恢复不经此路径，档内值语义不变）。

note 3 项（溢出双检防御、日终资格集合重建量、25 处机械补字段）经复核
认定可接受/必需，不改。修复轮验证：engine 模块 13/13、Web 相关 67/67、
tsc 0 错、oxlint 0 警告。

## 已知边界与遗留

- **名册日结回执线性增长**：每个交易日每名册为全体持有人落一条日结回执（含零
  变动条目，既有口径），默认 Web 局（5 股 × ~2 万持有人）量级约 5MB/交易日；
  未做回执压缩，长局存档体积为已登记边界（trading-rules 同节），留待后续批次。
- `AShareIndividual` 模式下 ~2 万散户 × 5 股的全量个人税账（每账一 lot、
  逐日日结事实）同步放大存档体积；简税默认局不建税账不受影响。
- 后入股东钩子只在公开市场日净额路径触发；非交易过户（送转/配股）只作用于既有
  持有人，不产生新入册账户（既有语义）。
- UI：`auto_corporate_foundation` 未暴露新局编辑面（产品默认开，工程开关随档
  固化）；面值输入已接线（ParValueInput）。

## 与并行批次的冲突面（如实登记）

- F 契约批（并行）：`company/system.rs` capabilities、`company/api.rs`、web
  `host/engine-host.ts` 查询面——本批未触碰。
- N2b（后续）：session 命令面与 web 面板——本批改 `session.rs` 装配段与
  `session/corporate_actions.rs` 日终段，N2b 接线时注意 `close_registries_through`
  新参数与 `SessionSetup` 新字段。
- 存档契约：`SessionSetup` 两新必填字段（旧档显式拒绝）、corporate_actions
  registries 非空、company_system legal_facts 非空、config preferences 温和默认
  ——四份 fixture 已重生成，Web 消费者测试已同步。
