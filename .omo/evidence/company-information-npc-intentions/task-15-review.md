# Task 15 Independent Review — 定期报告、临时公告和不可变公开信息库

VERDICT: APPROVE

- Commit under review: `da6c399` (`feat(engine): 增加财务披露与公开版本库`), branch `codex/feat/web-ui-polish`,
  parent chain `0a58b65 → 9978534 → ceadac7 (task-13 restatement fix) → 52b0d16 (task-13 docs) → da6c399`.
- Reviewer: independent subagent (did NOT implement task 15). Execution fully isolated in
  `git worktree add C:\Users\msuad\AppData\Local\Temp\opencode\wt-review-15 da6c399` (detached HEAD); no cargo/git-write
  was run in the main tree. Raw outputs: `C:\Users\msuad\AppData\Local\Temp\opencode\t15r-{1,2,3,4}.txt`
  (+ `t15r-doc-diff.txt`, `t15r-accounting-full.txt`).
- Files read in full: `src/information/{mod,publication,schedule,public_view,queries,prehistory}.rs`
  (160/315/160/219/116/188 lines), `src/session/disclosures.rs` (198), `session.rs`/`lib.rs` diffs (+5/+2),
  all 11 test files under `tests/publications/` (main 21, fixture 250, books_fixture 95, session_fixture 67,
  schedule_gold 146, correction_gold 182, prehistory_gold 228, weekend_publish 281, failures/mod 192,
  publication_failures 228, announcement_failures 72), evidence `task-15-{happy,failure,full-suite}.txt`,
  notepad issues.md (#783–824) / learnings.md (#848–911), plan task 15 + K4 anchors,
  `docs/company-accounting.md` @ da6c399, `tests/fixtures/company-model/policy-sources.json`.

## AGENTS.md 三问

### 1. 是否符合大 A 语义，依据是否可靠

符合，且依据链诚实。逐项：

- **排期红线（K4）**：`schedule.rs` 基准日 年报次年 3-20 / Q1 4-20 / 半年 8-15 / Q3 10-20，18:00 相位
  （`PHASE_SECOND = 18*3600`），公司稳定偏移 0..=7 自然日（逐日 `next()`，不跳交易日）；偏移由
  `stable_company_offset` = FNV-1a(tag+公司id) ⊕ seed 过 SplitMix64 终结子纯函数派生（任务 14 `OperatingRng::derive`
  同算法孪生，无状态重 derive，从不推进）。契约守卫：年报 ≤ 次年 4-30、半年 ≤ 8-31（证监会令 182 号第十三条，
  任务 2 已核验原文）；Q1 严格晚于上一年年报（`q1 <= prior_annual` 即拒）。`schedule_gold.rs` 把全部红线钉死：
  基准日逐字、偏移 7 的具体日期（2032-03-27/2031-04-27/2031-08-22/2031-10-27）、全域 0..=7 × 抽样年份
  （2000/2029/2030/2097）窗口合法 + 年报先于 Q1 + 季度节奏有序、偏移 8 类型化拒绝。Q1/Q3 无法定校验来源
  （fixture `sse/szse-listing-rules-current` blocked）在代码头与 docs §7 诚实声明"不声称法定"。
- **披露时序（K4）**：`disclosures.rs::run_day_end` 承接 finalize 之后的 18:00 相位（公告先于定期报告；
  公司 id × 种类确定序）；临时公告 `ensure_announcement_timing` 强制公布日 == 发生日（= 发生后的下一个 18:00
  相位，本引擎当日业务在 18:00 前 finalize），早于发生日/相位外/晚一天分别类型化拒绝。公告内容
  `AnnouncedEvent{kind, amplitude_bp, starts_on, expires_on}` 只含激活时已确认的冲击条款，类型上无
  「已实现」标记位——未来合同现金流不可标已实现（K4 明文）。任务 14 review 的 F-O1（银行/保险惰性事件
  不得叙述成经营事实）：派发只遍历 `economy().active()` 且 `starts_on == settled`，惰性事件无公告路径，✓。
- **PublishedReport 契约（K4）**：公司（`company`）/范围（`reports.scope` 单体|合并）/期间（`reports.period`）/
  会计政策（`policy.chart_version` 最小承载）/批准+公布时点（`approved_at`/`published_at` 显式分离）/
  版本（`reports.version`）/来源+更正关系（`origin` ⟺ `supersedes`）/报表+附注（内嵌任务 13 `ReportSet` 五产物，
  单一真源，恢复边界零一致性检查负担）。不可变（无 mutation API、BTreeMap 只插不改、无删除路径）、按 ID 查询
  （`queries.rs`，`EarlyRead` 提前读取守卫）。普通查询面只见公开版本，不见未披露总账（`publish_closed` 只从
  结账登记簿取定稿版本）。公开更正 = 新条目 supersedes 旧 ID，`correction_gold` 证明旧版本查询逐字节不变、
  登记簿镜像字节相等、latest 按 as_of 翻转。只有登记簿中勾稽通过的版本可公开
  （`ReportNotFinalized` + `set.validate()` → `ReportNotPublishable`）。
- **前史装配**：`assemble_seeded_prehistory` 调任务 14 `generate_history` + 任务 13 报表构建/登记，
  只纳入 `instant < 开局日 00:00` 的排期，`SeededPrehistory` origin 标记 + 恢复边界重验排期吻合；
  2000-01-01 最早开局 → 1998-01-01 前史首日（1998 初始化下界遵守）+ 360 交易日预置 K 合法
  （`prehistory_gold::earliest_start_prehistory_is_legal_and_restorable` 实测 14 期 = 2 公司 × 7 期：
  1998 Q1/H1/Q3/年报 + 1999 Q1/H1/Q3；1999 年报（2000-03 公布）与 2000 Q1 不提前纳入）。
  比较项诚实性锚定任务 13 固定语义：1998 年报上年比较 Available（开局凭证 1997-12-31）、1998 中期
  Unavailable（上年同季窗口早于凭证）——与 ceadac7 修复后的窗口级判定一致。
- **correction_gold 与 task-13 修复（ceadac7）一致性**：金样走 `closing.correct` 在开放期间 2031-01 过账
  更正分录、目标闭期 2030-12 年报——正是 ceadac7 修复的跨期泄漏场景；断言净利差恰为 +300.00 元、登记簿 v1
  字节不变。与修复后语义一致，✓。

### 2. 改动是否为需求所必需、是否保持最小范围

是。21 文件全部为 engine（+1 行 docs 算术笔误修正：company-accounting.md L196 「年报最晚 4-27」→「3-27」，
3-20+7=3-27，正确且与本提交主题直接相关）。`session.rs` +5 / `lib.rs` +2 纯模块声明与再导出，无既有行为改动
（extraction_replay 3/3 绿）。无删除、无重构既有代码、无越层依赖（information → {accounting, calendar, company}，
被 session/disclosures 消费，方向符合架构）。无投机性抽象；`AccountingPolicyRef` 只含 chart_version 是显式
最小承载决定（issues #5，任务 16/29 需要时再扩）。两处中途污染的过渡提交已被 soft-reset，最终历史干净。

### 3. 是否存在遗漏的边界测试、跨层语义漂移或不必要复杂度

计划点名的四类拒绝全部有类型化测试（早读：报告+公告双面；未结账：序号越界+期间未快照双路；不平报表：
恢复边界篡改 +1 分；时间逆序：公布早于批准+批准落期内；另有非法窗口、相位/排期偏离、偏移越界、重复 ID/
域外 ID/幻影计数器、更正关系五路错配、公告三路时序、缺 `published_at` 字段显式反序列化失败——共 10 个
failure 用例，全部先检查后变更或失败即弃建，库不外泄半建成状态）。闭环测试覆盖：周末（休市日）18:00 公布
零市场副作用（事件 seq/交易日/tick/快照字节逐位不变）、恰好一次重派发 no-op、同 seed 前史 ID 序列+库 serde
字节逐位一致、serde 往返字节稳定、发布历史不可覆写。未发现跨层语义漂移（时点/排期/勾稽校验在 publish 与
恢复边界同族函数 `ensure_report_shape` 复用）。复杂度受控：无 f64/f32 进入公布身份/ID 数
（grep 证实 information/ 与 disclosures.rs 零浮点；PublicationId 为 u32 单调共享计数器，恢复边界强制与最大 id
严格衔接）。见「非阻断观察」三条，均不构成缺陷。

## 计划 K4 / 验收探针逐项结论

| 探针 | 结论 | 证据 |
|---|---|---|
| 排期红线（基准日/偏移/相位/Q1 序/休市日发布不推迟） | ✓ | schedule.rs L74–159；schedule_gold 全 5 测试；weekend_publish（周六 Q1 真实发布，零撮合零事件零 RNG） |
| Q1 不早于上一年年报守卫 | ✓ | schedule.rs L145–156（严格 >）；schedule_gold::all_windows_legal_and_annual_precedes_q1 |
| 日终顺序 finalize→（封账）→18:00 披露 | ✓（封账见 D-2 裁决） | disclosures.rs run_day_end；weekend_publish 两日场景按序调用 |
| 临时公告下一个 18:00、只含已确认事实 | ✓ | ensure_announcement_timing；AnnouncedEvent 无已实现位；interim_announcement 金样 |
| PublishedReport 不可变/按 ID 查询/更正新版本关联旧 ID/历史不可覆写 | ✓ | public_view.rs 无删除路径；correction_gold 字节级断言 |
| 只有定稿且勾稽通过的报告可公开 | ✓ | publish_closed：closing.version → ReportNotFinalized；set.validate → ReportNotPublishable；两个 failure 用例 |
| 前史以真实排期组装、未来报告不提前纳入、SeededPrehistory 标记、2000-01-01 开局 | ✓ | prehistory.rs `< start_instant` 过滤；prehistory_gold 3 测试 |
| 失败电池（早读/未结账/不平/时间逆序/非法窗口/重复 ID）各 typed、状态不变 | ✓ | failures/ 10 用例；错误先于任何变更返回；from_parts 失败即弃建 |
| 同 seed 排期/ID 一致；版本保存后不变 | ✓ | stable_company_offset 纯函数；same_seed_prehistory_ids_and_bytes_identical；correction_gold 字节不变；serde 往返字节稳定 |
| 披露窗口休市日零市场 RNG/tick | ✓ | weekend_report_publishes_without_trade：seq/day/tick/快照 JSON 全等 |
| 无 f64 泄漏；ID 稳定（序列制） | ✓ | grep 零浮点；u32 单调 + from_parts 严格衔接校验 |
| F-O1：公告不含惰性事件叙述 | ✓ | 只遍历 active() 且 starts_on==settled |
| extraction_replay 保持绿 | ✓ | 全量套件 3/3 |

## 注册登记核查（第 6 执行项——重点探针）

da6c399 的 stat：21 文件全 engine + 1 行 docs；零 policy-sources.json/fixture 改动。裁定：**本提交只依赖
既已注册的假设 + 计划 K4 文本，无未登记的新增简化需要在本提交补注册**，具体：

- K4 排期四日期 + 偏移 + 18:00 + Q1 序 + 休市日发布：**已预注册** `game-assumption-report-schedule`
  （policy-sources.json L502–513，登记日 2026-09-10，早于本提交；issuer「计划 K4，用户已批准」）。
  da6c399 实现的内容逐条落在该假设的 game_assumptions 文案内。
- 季报/半年报快照语义的会计侧依据：`cas-32-interim-2006` 保持 **blocked** 状态（"K4 季报/半年报快照语义的
  会计侧依据；任务 15 实现前须补原文"），fixture 未假装已取证；报表/结账口径简化由
  `game-assumption-reporting-closing-simplifications`（D1–D7，任务 13 登记）承载。与任务 13 在 cas-28/31
  blocked 下推进并登记 D1–D7 的先例一致。
- 临时公告内容分类学 = 结构化已确认事件条款：计划 K4 L119 原文即此语义（用户批准的计划文本），非新增未登记简化。
- 新增游戏约定 `APPROVAL_HOUR = 8`（批准时点 = 公布日 08:00）：计划未规定批准钟点，属游戏自设表示细节
  （无对应真实规则被简化掉；真实规则也不固定"批准时刻"）。代码内单一常量 + `游戏约定` 文档注释 + notepad
  learnings 登记。见观察 O-2（建议任务 16/18/29 消费该字段时并入 game-assumption-report-schedule 文案），
  不构成门禁违反。
- docs 唯一改动是修正已注册假设旁的算术笔误（4-27→3-27），未引入新语义。

## 偏差裁决（实施者已登记，独立复核）

- **D-1 文件清单偏离（接受）**：计划字面 `information/{mod,publication,schedule,public_view}.rs`；实际 6 文件
  （+queries.rs 查询面、+prehistory.rs 前史装配；mod.rs 存在如计划）。合并版 public_view 将达约 423 纯逻辑行
  （实施者口径；拆分后三文件纯行 181+87+148），超 250 纯行天花板——拆分沿职责边界（库核心/查询面/开局装配），
  `pub(in crate::information)` 字段可见性有 `pub(in crate::session)` 先例。测试侧 12 文件（failures 拆 3、
  fixture 拆 3）同因（397/356 行单体超限）。QA 命令 `--test publications` 不变。与 task-7/8/11 文件数偏离先例
  一致，均已登记（issues #1）。裁决：天花板优先于计划文件清单字面，接受。
- **D-2 月/年末封账未接入日终（接受，已移交任务 26）**：K4 日终顺序的「封账」一步结构性不可达（行业账套只暴露
  只读 `books()`，close_month/close_year 需 `&mut Books`）。本任务以 ClosingEngine 登记簿承载「定稿可公开」
  （Q/H1 走 snapshot_interim、Annual/Monthly 走 generate+record，均经勾稽+比较项诚实性校验且不可变）。
  issues #2 显式登记并移交任务 26 扩 `books_mut()` 或接线时处理——非静默缺省，且未产生与 K4 相矛盾的公布
  （无登记版本仍不可公开）。接受。
- **D-3 fn 指针观察者边界（接受）**：任务 5 的 `DisclosureObserver = fn(CivilInstant)` 无捕获，有状态派发不可能
  在观察者内执行。生产观察者为无状态相位钩子（panic-free 保持任务 5 复核 N2），状态性派发由
  `DisclosureDispatch::run_day_end` 在同一相位瞬间以 `CivilDayEndReport::disclosure_instant` 为权威执行。
  诚实声明（issues #3），架构自洽，接受。
- **D-4 排期契约守卫不可达路径（接受）**：年报 ≤4-30、半年 ≤8-31、Q1 晚于上年年报三守卫在当前基准表
  （3-20/4-20/8-15/10-20 + 等偏移 ≤7）下数学恒满足（不可达拒绝路径）。保留依据成立：基准表是运行时常数，
  漂移时显式失败优于静默违规（铁律二）；正向语义由 all_windows_legal_and_annual_precedes_q1 金样锁定，
  偏移域拒绝（offset 8）可达且已测。任务 21 教训以「正向锁 + 契约守卫」组合回应，可接受。
- **D-5 一性化未复现失败（接受，如实登记）**：实施者首跑全量出现 1 个 insurance_accounting 位置失败（测试名
  未捕获），随后两次全量 + 隔离重跑全绿，按环境抖动登记（issues #7）。本次隔离复跑 insurance_accounting 18/18
  绿、全量 0 失败，支持抖动判定。若复现再升级排查。
- **D-6 LOC 天花板（通过，无需例外）**：纯行口径（非空/非注释/非属性/非 use）：publication.rs 315 原始 →
  **232 纯行**；weekend_publish.rs 281 原始 → **234 纯行**（单场景例外条款无需动用）；fixture.rs 250 原始 →
  210 纯行；其余全部 ≤188。实施者自报最大 238，与本次独立口径（±6 计数规则差）一致。全部 ≤250，通过。
- **D-7 information/ 未加 ts_rs derive（接受）**：宿主 DTO 归任务 29，避免生成物扰动；已登记（issues #6）。

## 隔离复跑结果（本人执行，worktree @ da6c399）

| 命令 | 退出码 | 结果 |
|---|---|---|
| `cargo test -p engine --test publications` | 0 | **22 passed / 0 failed / 0 ignored**（2.18s），测试名与 E/task-15-happy.txt 逐条一致 |
| `cargo test -p engine` | 0 | 33 套件合计 **815 passed / 0 failed / 4 ignored**（ignored = 既有 release 压力门 20k/50k/100k 账户 + 2 万散户成本探针，均先于本任务）；doc-tests 0；与 E/task-15-full-suite.txt 逐套件计数一致（其总和亦为 815/0/4） |
| `cargo check -p engine -p server -p web-wasm -p engine-gpu` | 0 | Finished，0 警告 0 错误 |
| `cargo clippy -p engine --all-targets` | 0 | 7 条警告全部先于本任务：`behavior/decision.rs:239` unnecessary_filter_map（任务 1 起登记，指令明示不据此拒绝）+ `tests/experience_feedback/main.rs` 41/65 too_many_arguments、187 bool_assert_comparison + `tests/analysis_profiles/invariants.rs` 40/57/212 unusual_byte_groupings。**information/、session/disclosures.rs、tests/publications/ 零警告** |

计划 QA failure 面（早读/未结账/不平/逆序/非法窗口/重复 ID，不以补默认日期绕过）：`missing_instant_field_is_a_
deserialize_error` 直接钉死「无默认日期」；其余 9 用例与 E/task-15-failure.txt 一致（10/0，过滤参数复跑三段）。

## 非阻断观察（备案，无需返工）

- **O-1（记录性）** learnings.md 末行「全量 814 过/0 败/4 忽略（基线 791 + publications 22）」为算术笔误：
  证据文件与本次隔离复跑均为 **815**（且 791+22=813 自身不合）。仅 notepad 记录口径，代码/证据无误。
- **O-2（建议）** `APPROVAL_HOUR=8` 批准时点约定建议在任务 16/18/29 首次消费 `approved_at` 字段时并入
  `game-assumption-report-schedule` 的 game_assumptions 文案（届时 fixture 版本化一并处理）。
- **O-3（风格）** `insert_report` 的重复 ID 检查是「先 insert 后查 Some」——重复路径上旧条目已被替换才报错。
  不可观测：publish_closed 分配的 id 恒新鲜；from_parts 失败即整体弃建，半建成库不外泄。改 `contains_key`
  先查更整洁，非缺陷。

## 结论

三问全部通过：大 A 语义符合且依据链（法定核验 + 预注册游戏假设 + blocked 诚实登记）可靠；改动最小且未越界；
边界测试无遗漏、无语义漂移、无不必要复杂度。全部计划验收句有真实测试锁定，四条隔离命令 0 退出码，
证据文件与复跑逐条吻合。已登记偏差（D-1…D-7）均有成立理由且非静默。**APPROVE**。
