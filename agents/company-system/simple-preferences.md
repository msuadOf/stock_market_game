# P 批：simple 模型公司行为偏好自动提案（2026-10-07）

## 范围与依据

- 产品决策：ADR-0037（偏好自动方案与显式 API 并存；公司决策不由投资者发起）+
  2026-10-07 用户补充（见 ADR-0037「实现批次用户决策注记」：偏好属模型内部、
  simple 最简、性能占用少）。
- 实现落点：`packages/engine/src/company/simple/preferences.rs`（类型 + 纯评估函数 +
  拒绝台账）；共同层（`company/system.rs`）只加最小接口（`settlement_completed_on`、
  `simple_preferences`、`record_preference_rejection`、`last_preference_rejection_on`、
  `preference_rejections`）；Session 日结（`session.rs`
  `end_civil_day_after_session_check` 在 `advance_day` 后）负责评估接线并把提案送进
  既有 `approve_cash_dividend` / `approve_stock_distribution`（同一状态机，零旁路）。
- 覆盖范围：现金分红与送转两个偏好项；配股/增发/回购仅结构扩展位（M 批机制在飞）。
  自动送转固定 `BonusShares`（转增缺资本公积账面且库藏股要求回购专户事实）。

## 关键口径（详见 docs/trading-rules.md「公司行为偏好自动提案」）

- 未配置（两项 `None`）= 无偏好 = 不自动产生方案；结构前置不满足（未上市、无完整
  名册）不评估不记录。
- 提案节奏：批准日 = 公告日 = 周期末日次日；登记日为公告日后首个交易日且不与公告日
  同日（否则 typed 公告在披露时已推进为 Registered 而永久错过公告通道——本轮红→绿
  发现并修复）；除息/除权 = 登记日次一交易日；派发 = 除息日。
- 游戏化派息封顶：每股派息按「实际日程推导的跌停敞口余量」封顶（敞露交易日数 n ×
  涨跌幅限制推导的最坏登记日收盘下界，合并同除息日既有方案 gross；修复轮改写，
  详见下文「门禁复核修复轮」与 `docs/trading-rules.md`），防止除息参考价非正令除息日
  日结致命失败（偏好自动方案不得把局推向不可结算状态）。
- 拒绝台账（`SimplePreferenceLedger`）：记录公司/评估日/类别/原因；同键同因幂等、
  同键异因显式报错；接受事实由 finance 分红/送转事实与 Session 账簿承载，不重复。
- 频率：`cycles_between_proposals` 按自然月差 × 结算周期月数计算完整周期间隔；上一
  次提案（接受按批准日前一日归并周期，被拒按台账评估日）起算。
- 确定性幂等键：`simple-preference:{company}:dividend:{period_end}` /
  `simple-preference:{company}:stock-distribution:{period_end}`。
- 严格持久化：`SimpleCompanyConfig.preferences` 与 `SimpleCompanyState.preference_ledger`
  均无 serde 默认；旧档缺字段显式拒绝（对齐 `dividend_tax_mode` 先例）。

## 红绿日志索引

红（新 API 尚不存在，编译期缺失 + 行为断言失败，均在本 worktree 开发过程中实际发生并
由此后实现转绿）：

1. 首次编译红灯：`preferences.rs`/`SimpleCompanyConfig.preferences`/
   `SimpleCompanyState.preference_ledger` 不存在 → `cargo test --no-run` 编译失败
   （E0515/E0271/E0308 等，见开发记录）。
2. 行为红灯 A：`cash_evaluation_floors_per_share...` 断言失败——评估函数把「税前总额」
   误传入 `gross_per_share`（Session 全链路测试以
   「决议总额必须等于完整名册中非库存股的税前每股金额乘以股数」拒绝记录形式捕捉），
   修复为独立的每股/总额双值。
3. 行为红灯 B：全链路测试在除息日以「前收盘价减税前每股现金红利后参考价必须为正数」
   致命失败 → 引入派息封顶（游戏化保护）。
4. 行为红灯 C：全链路测试断言 typed 公告缺失（library announcement_count == 0）→
   公告/登记同日导致披露时状态已 Registered，改为登记日不与公告日同日。

绿（命令均带 10s 外部 deadline，Rust 组内 8/9 线程；日志在
`.tmp/company-system/simple-preferences/`）：

- `engine-green.log`：`company::simple` 84/84（含偏好单元 12/12 与 500 公司性能用例）、
  `session::simple_preferences_session_tests` 9/9、`session::corporate_actions` 12/12、
  `session::dividend_tax_mode_tests` 8/8、`session::company_simple_session_tests` 36/36、
  `information::source_tests` 10/10、`company::dividend_contract_tests` 1/1。
- `cargo check --workspace --all-targets --exclude stock-market-game` 通过
  （`stock-market-game` desktop crate 需先构建 `apps/web/dist`，属既有环境前置，与本批无关）。
- typegen：`cargo test -p engine export_bindings` 生成
  `SimpleCompanyPreferences` / `SimpleCashDividendPreference` /
  `SimpleStockDistributionPreference` 并更新 `SimpleCompanyConfig`（未手写）。
- `web-system-schema-green.log`：Web `system.test.ts` 16/16（含偏好正负例与台账负例）；
  `tsc -b` 与 oxlint（改动文件）通过。
- `web-full-batch.log`：完整 Web 批剩余 12 个失败均为与本批无 import 关系的既有失败
  （public-financials-render 利润表用例为 handoff 已登记 main 既有失败；Tauri/Worker/
  WASM runtime 组的 mock 形状与 Node 25.8.2 环境差异疑点已逐文件核对未触碰本批文件）。
- fixtures：`cargo build --release -p engine` 后 rustc 直连 rlib 并行编译三个 producer
  （minimal/closed-day 为既有 checked-in generator 补 `preferences` 字段；main 为本批
  新增 `main-save-fixture-generator.rs`，五股显式 setup 对齐 Web DEFAULT 场景），各自
  内置守卫 + `ProtocolSession::restore` 后 resave 深等通过后安装
  `current-schema-save.json`、`current-closed-day-save.json`、`minimal-current-save.json`
  与 `current-company-slice.json`（主档 `company_system` 精确投影）。

## 已知边界与遗留

- 偏好配置 UI 入口（新局/装配期编辑）与台账 UI 呈现归后续批次（先例：股息税
  outstanding views）。
- `company-system-config.test.ts` 既有 4 项失败（inline fixture 缺 `kind`、期望科目与
  DEFAULT_SETUP 漂移）为本批之前即存在的 main 基线，未顺手修复以免混入无关改动。
- M 批（配股/增发/回购）在另一 worktree 平行开发；`SimpleCompanyPreferences` 的扩展位
  注释即为其预留，落地时按 ADR-0037「偏好自动 + 显式 API」双入口接入。

## 门禁复核修复轮（2026-10-07，fix-round）

P 批门禁复核 findings 的修复记录（复核意见全文见主对话；下述按 finding 归纳）：

### major：派息封顶跌停敞口（已修）

- **缺陷确认**：原封顶「每股 ≤ 收盘价 − 1 分」在提案时点取锚，而除息日
  `prepare_ex_references_for_current_date`（session/failure.rs）用**登记日**收盘算
  `previous_close − gross_per_share`；公告→登记间 1–2 个交易日内跌停幅度内的下跌即可把
  参考价压到非正 → `NonPositiveReferencePrice` → StepFatal → poison 阻断存档，自动提案
  每周期重试必然复现。文档宣称「防止不可结算」与实际保护不符。
- **修复口径**（`company/simple/preferences.rs` + `session.rs` 接线 +
  `docs/trading-rules.md` 同步改写）：
  1. 封顶改为按**实际日程推导的跌停敞口余量**：敞露交易日数 n = `(周期末, 登记日]`
     内的交易所交易日（本日程结构下恒为 1 或 2，公告日休市时 1——禁止固定 2 天），
     封顶 = floor(锚收盘 × (1 − n×limit_bps/10000)) − 必留正参考价分子；余量耗尽
     （n×limit ≥ 10000，现行板块限制下不可达，防御性拒绝）或封顶非正时如实记录拒绝。
     数学依据：单日跌停价 = 昨收×(1−p) 四舍五入（偏上）且 ≥ 1 价位，n 日连续跌停后
     收盘 ≥ 锚×(1−p)^n ≥ 锚×(1−np)（Bernoulli），线性下界向下取整只会更保守。
  2. **同除息日合并口径**：既有方案（显式 + 自动）的 `gross_per_share` 求和后必须仍
     满足封顶；同除息日存在送转事件时分子须保留 `1 + ceil(ratio/10^6)` 分（合并除权
     参考 = 分子/(1+ratio) 银行家舍入到分，商 0 时舍入不保证 ≥1 分）。Session 侧
     （`same_ex_date_preference_context`）按本周期日程推导除息日并扫描既有账簿。
  3. **送转侧同族保护**（同一「自动提案不得把局推向不可结算状态」不变式）：同除权日
     已存在送转事件 → 自动送转提案拒绝（两起送转合并除权口径未核实，叠加会在除权日
     日结显式失败）；同除权日现金红利合并后余量不足 → 拒绝。现金偏好先评估、送转后
     评估，同周期刚批准的自动现金方案在送转评估时已入账簿、计入合并（见代码注释）。
  4. `target_payout_bp` 配置域**维持 1..=10000bp 不加严**：价格安全已由日程余量封顶
     保证；派息比例另受「决议总额 ≤ 可分配利润」制度校验约束，收紧 bp 域只会无据
     限制玩法，不消除任何真实风险。
- **红→绿证据**（均在 `.tmp/company-system/simple-preferences/`，命令带
  `run-with-deadline.mjs 10000` 外部 deadline）：
  - 红：`fix-round-red.log`——4 个新 Session 用例在旧代码上全部行为红：
    A 连续跌停场景旧封顶 999 ≠ 799（即敞口缺陷本体）；B 同除息日显式方案占满余量时
    旧代码照样批准第二个方案（0 条拒绝记录）；C 旧代码不合并 gross（999 ≠ 299）；
    D 旧代码自动送转与显式送转同除权日叠加（2 本账簿）。
  - 绿：`fix-round-green-session.log`（Session 偏好 13/13：9 既有 + 4 新增）、
    `company::simple` 86/86（偏好单元 14/14 含新增敞口/合并边界与 500 公司性能用例）、
    Session Simple 36/36、`corporate_actions` 12/12、`dividend_tax_mode` 8/8、
    `market::` 8/8、`fix-round-cargo-check.log`（workspace --exclude stock-market-game
    通过）、`fix-round-web-system.log`（Web `system.test.ts` 16/16；相邻 schema 组
    simple-finance/reports/annual-growth 13/13）。
  - 新增 Session 边界用例：`auto_proposal_at_cap_survives_consecutive_limit_downs_to_ex_date`
    （按封顶生成后公告→登记**真实连续一字跌停**（每敞露日以机构挂单+玩家卖出在跌停价
    成交）走到除息日，断言参考价 = 810−799 = 11 分为正、当日再日结到 Paid、全程无
    poison）、`auto_proposal_rejected_when_same_ex_date_explicit_plan_exhausts_margin`、
    `auto_proposal_merges_same_ex_date_explicit_gross_into_cap`（合并后 500+299=799 恰
    回封顶）、`auto_stock_proposal_rejected_when_same_ex_date_explicit_event_exists`。
    测试侧日程/敞口计数独立实现（逐自然日查日历），不与实现共用推导。
  - 完整 `engine --lib` 回归：`fix-round-engine-lib-full.log` 1469 passed / 257 failed；
    stash 复跑基线 1463/257——**失败集与本批无关且数量一致**（该 worktree 基线即有
    257 项失败，P 批原台账只登记了定向组），本修复 +6 通过、0 新增失败。
  - 过程事故（如实登记）：曾对本批 5 个 Rust 文件单独运行 `rustfmt`，其默认递归
    格式化了 `session.rs` 引入的全部子模块共 49 个非目标文件；已逐一 `git checkout`
    还原为仅保留本批 7 个目标文件的改动，还原后全组测试与 cargo check 复跑通过
    （`fix-round-green-final.log`）。未执行被禁止的 `cargo fmt --all`。
- **文档**：`docs/trading-rules.md`「公司行为偏好自动提案」封顶条目按上述口径改写，
  含除息日当日跌停为何安全（安装的参考价即当日昨收，跌停下限按昨收×(1−p) 四舍五入
  且最低一个价位，恒 ≥1 分）与剩余边界（显式 API 不受游戏化封顶；两起**显式**送转
  同除权日仍显式失败——既有显式侧行为，未改变）。

### note：性能（已修）

`session.rs` 每公司每周期末 `dividend_plan_facts`/`stock_distribution_facts` 原各取两次
（幂等判定一次、频率归并一次）且各克隆一个 Vec——改为每公司每类别**只取一次**并在
两处复用（`last_simple_proposal_period_end` 改收 `approved_on` 迭代器）；行为不变，
`company::simple`（含 500 公司性能用例）与 Session 组全绿佐证。

### note：`last_simple_proposal_period_end` ±1 天归并口径（维持，登记不改）

接受的提案按「批准日的前一自然日」就近归并周期末：显式方案批准在周期中段时会把
归并日提前到上一周期（例如 01-21 批准 → 归并 01-20，属 1 月；若上一周期末为 12-31
则归并错周期一日）。复核认定该口径只影响频率判定早晚一个周期、无正确性影响
（提案间隔语义本就是「至少间隔 N 个完整周期」的保守近似），**维持不改**；同月内
显式批准会抑制同周期自动评估的连带行为已在新 Session 用例中固定（见 B/C 用例注释）。

## 与 M 批的预期冲突面

- 存档契约与 fixtures（`SimpleCompanyConfig`/`SimpleCompanyState` 新字段、四份 fixture JSON）。
- `agents/company-system/current-handoff.md`、`implementation-checklist.md`、
  `docs/trading-rules.md`（相邻章节）。
- `packages/engine/src/session.rs` 日结段（M 批预期也在 `end_civil_day_after_session_end`
  附近接线）；`corporate_actions.rs` 本批未改（偏好状态全部落在模型内部，刻意缩小冲突面）。
