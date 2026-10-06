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
- 游戏化派息封顶：每股 ≤ 最新收盘价 − 1 分，防止除息参考价非正令除息日日结致命失败
  （本轮红→绿发现的第二个真实缺陷：偏好自动方案不得把局推向不可结算状态）。
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

## 与 M 批的预期冲突面

- 存档契约与 fixtures（`SimpleCompanyConfig`/`SimpleCompanyState` 新字段、四份 fixture JSON）。
- `agents/company-system/current-handoff.md`、`implementation-checklist.md`、
  `docs/trading-rules.md`（相邻章节）。
- `packages/engine/src/session.rs` 日结段（M 批预期也在 `end_civil_day_after_session_end`
  附近接线）；`corporate_actions.rs` 本批未改（偏好状态全部落在模型内部，刻意缩小冲突面）。
