# Learnings — company-information-npc-intentions

Conventions, patterns, and successful approaches discovered during work on this plan.

_Auto-scaffolded by /start-work. Append new entries below - never overwrite._

---

## 2026-09-10 W1-Task 1：变更前多 seed 基线（before 锚点）

- **当前合法 setup 的真源**是 `apps/web/src/config/defaults.ts` 的 `DEFAULT_SETUP`（5 股
  600101/002156/300260/600610/000812、20000 散户、5 机构、2 游资、15300 tick/日 =
  900 开盘窗口 + 14400 连续 + 180 收盘集合）。`packages/engine/examples/baseline_fixture.rs`
  的 matrix 场景是它的逐字段 Rust 副本；engine 单测钉住股本/时钟防漂移。压缩成本场景
  的真源是 `packages/engine/tests/session.rs` 的 `large_retail_account_setup(20_000)`
  （300 tick/日、无竞价窗口、100M/80M 股本、`GameConfig::proposed_defaults()`）。
- **默认 setup 的当前真实行为包含非零 `engine_error_events`**（SettlementError/VError
  计数）：matrix 场景 30 日每 seed 55–98 个（compressed-300 为 0）。`docs/diagnostics.md`
  的契约是报告该计数而非拒绝；before 锚点已逐 run 记录进 manifest 并在日志显式警告。
  后续任务 38/42/F4 做 before/after 对比时必须把该指标当作观测项而不是失败项。
- **运行成本实测**（Ryzen 7 5800U，16 线程，release）：matrix（15300 tick/日 × 30 日 ×
  20k 散户）每 seed 约 270–500 s；compressed-300 每 seed 约 105–111 s；完整 before 采集
  （10 seed + 首 seed 确定性重跑 × 2 场景）约 80 min。单 seed 报告约 64–66 KB/文件。
  cargo release 首次编译 engine 约 2 min。规划后续 after 采集时按此预算。
- **确定性**：同 seed 同配置 stdout 逐字节一致（sha256 复核通过）；Rust `println!` 在
  Windows 上只写 `\n`，跨平台字节稳定。
- **PowerShell 5.1 证据采集坑**：PS 的 `>` 重定向会把子进程 stdout 重编码为 UTF-16（文件
  直接损坏）。字节级证据必须用 `cmd /c "... > file 2>&1"` 或在 node 内部写盘。控制台
  中文乱码只是显示问题。另：PS 管道中 `$LASTEXITCODE` 偶发 -1（Select-String
  NativeCommandError 干扰），判定成败要用 `cmd /c` 包一层再看退出码。
- **pnpm 不在 agent shell 的 PATH**（node 24.18 / cargo 1.96.1 正常）。manifest 采集时
  必须把不可用显式记入 `blocked`（baseline-run.mjs 已实现），不能静默省略。
- **脚本测试模式**：任务限定恰好 3 个产品文件，无法按 `market-ui-report-lib.mjs` 惯例拆
  lib/CLI；改为 `baseline-run.test.mjs` 直接 import CLI 模块的导出（`captureBaseline`
  接受注入 `exec`，22 个用例不依赖 cargo，<0.3 s 跑完）。失败路径（非零退出码、缺报告、
  配置不符、摘要漂移、非空输出目录）全部有负向用例锁定。

## 2026-09-10 W1-Task 3：按责任接缝抽取 session/strategy/behavior（纯移动重构）

- **符号 → 模块映射**（后续任务 5/21/24/25/26 改这些接缝时直接查这里）：
  - `session/attention.rs`：`NpcAttentionState`、`market_attention_signal`、
    `effective_observation_probability`、`maximum_observation_probability`、
    `attention_candidate_is_observation`、`sample_attention_wait`、
    `GameSession::{pop_due_npc_ids, evaluate_attention_candidate}`、`attention_tests`。
    注意 `sample_attention_wait` / `maximum_observation_probability` 被 session.rs 的
    `populate_npcs` 调用 → `pub(super)` + session.rs 显式 `use attention::{…}`。
  - `session/views.rs`：`build_market_view`、`market_price_path_observations`、
    `behavior_market_observation`、`account_risk_observations_for`。
  - `session/self_views.rs`：`working_orders_by_account`、`build_self_view`(cfg(test))、
    `build_self_views_for`。
  - `session/snapshot.rs`：`MarketSnap/PositionSnap/AccountSnap/Snapshot` 类型 +
    `snapshot/runtime_snapshot/snapshot_inner`（serde `with` 路径改为 `super::` 前缀，
    字节不变）。`DailyCandle/DailyTradeStats` 留在 session.rs（candles 语义）。
  - `session/execution.rs` + `execution/{orders,reconcile,budget,records}.rs`：
    `ParentOrderPlan`、materialize/append、reconcile 对(+`ReconcileScope` 使用处)、
    `cap_npc_intents_to_available_cash`、`record_parent_order_{fills,submission,canceled}`。
    `WorkingOrderSlices/ReconcileScope/两个 type alias/PARENT_ORDER_HORIZON_MINUTES`
    留在 session.rs（跨接缝管道，step 与测试直接构造）。深度 2 模块的方法用
    `pub(in crate::session)`。
  - `strategy/`：mod.rs=契约(views/Intent/StrategyDecision/Rng/Strategy trait/StrategyError)+再导出；
    profile(风格/族/档案)、data(StrategyData+decide_data)、retail/institution/hot(纯内核)、
    sizing(a_share_tranche/a_share_sell_qty/risk_capped_buy_qty)、value(TargetPolicy+ValueStrategy+
    target_cents)、zi_noise、momentum(+InstitutionMomentumStrategy)、params、factory、sampling。
  - `behavior/`：mod.rs=公共类型+两个入口；decision.rs=decide_retail_position_inner
    （SIZE_OK：单函数 596 纯逻辑行，纯移动不拆函数体）；heuristics.rs=5 个支持函数。
- **跨兄弟模块私有字段坑**：`StrategyFactory` 直接改 `ZiNoiseStrategy/ValueStrategy/
  MomentumStrategy/InstitutionMomentumStrategy` 的私有字段；拆到兄弟文件后必须把这些
  字段放宽为 `pub(super)`（不进公共 API）。后续给策略加字段时注意保持这个约定。
- **子模块取父模块导入的 glob 传递**：`use super::*` 会把父模块的**私有 use 绑定**也
  带给子模块（persistence.rs 先例）。因此 session.rs 顶部 import 即使本模块不再直接用，
  只要子模块用就**不会**报 unused——不需要把 observation 导入搬到 views.rs。
- **纯移动重构的验证配方**（比"看 diff"强）：(1) 先在未动代码上跑 characterization
  测试并硬编码字节摘要（FNV-1a 即可，无需 sha2 依赖）；(2) 移动后摘要必须不变——
  本次事件流/日中存档/日终存档三个锚点全程未变；(3) 用脚本把 HEAD 区段与新文件
  做规范化逐字对比（剥离可见性前缀/serde 路径前缀后必须相等；rustfmt 换行的**尾逗号**
  是唯一合法差异）。20 项检查全过。
- **rustfmt 会因加 `pub(in crate::session)` 超宽而折行函数签名并加尾逗号**——逐字
  对比脚本要容忍 `,)` ↔ `)`。
- `cargo test -p engine` 遇到一个套件失败会**中止后续套件**（字母序在 policy_manifest
  之后的 session/strategy 被跳过），补跑要显式 `--test session --test strategy`。
- **cmd /c "... > file 2>&1 & echo EXIT:%ERRORLEVEL%" 拿不到真实退出码**（嵌套引号被
  PS 吃掉）；正确姿势：`cmd /c "… > file 2>&1"; echo $LASTEXITCODE`（cmd 退出码会传播）。
- rust-analyzer LSP daemon 在本仓库冷启动 30s 超时（3 次）；以 `cargo check` 0-warning
  + clippy + fmt 作为等价静态证据并如实记录。

## 2026-09-10 W1-Task 2：官方依据/会计政策登记的取证经验

- **官方正文获取的有效路径组合**：webfetch 直取可访问深页（kjs.mof.gov.cn、
  www.mof.gov.cn/zcsjtsgb 深页、sse.com.cn 规则深页含 docx 附件、csrc.gov.cn
  规章库含 docx 附件、docs.static.szse.cn PDF）+ curl 下载附件 + 本地提取。
  附件文本提取：`uv run` PEP 723 脚本（pypdf 提 PDF / zipfile+re 提 docx），
  **必须 `set PYTHONUTF8=1`**，否则重定向文件是 GBK 乱码。
- **PDF/docx 条款级取证完全可行**：CAS 14（22页）、CAS 22（37页）、证监会令
  182号（65条）、沪深交易规则全文均本地提取并逐条 grep 核对条款号；webfetch
  对 PDF 只回原始二进制，`look_at` 对 PDF 超时，Read 不支持 PDF（本模型）。
- **Bing 中文检索极不稳定**：仅最初两组 `site:kjs.mof.gov.cn + 标题` 查询有效；
  之后 site: 失效、全角文号（〔〕）致乱、返回完全无关结果。可靠替代：直接浏览
  官方栏目索引（kjs 政策发布、csrc 规章库 /csrc/c106256/fg.shtml、sse 页脚
  站点地图）。mof 规范性文件库（zcsjtsgb/gfxwj）档案只覆盖 2020-03~2020-07。
- **CAS 30（财会〔2026〕11号）分三层生效（2027/2029/2030）** 是 RegulationProfile
  "2030 默认开局直接适用 2026 版列报"的官方依据；同批通知均载明"允许提前执行"，
  支撑早期开局统一适用现行文本的游戏假设。
- **已被上位文核验的"存在性"可登记**：CAS 8/18/25/28/31/32/33/37 的现行效力
  经 CAS 30 各条交叉引用核验，文号/全文受阻仍可登记 blocked 而不虚报——
  fixture 状态三分类（verified-official / simulated-game-assumption / blocked）
  足以表达全部诚实度层级。
- **PowerShell 5.1 再添两个坑**：`Set-Content -Encoding UTF8` 写 BOM 毁 JSON
  （用 `[IO.File]::WriteAllText` + `UTF8Encoding($false)`）；cmd 内联
  `%ERRORLEVEL%` 在重定向链中提前展开，用 `cmd /v:on` + `!ERRORLEVEL!`。

## 2026-09-10 W1-Task 2 补充取证：官方档案的正确打开方式（纠正第一轮误判）

- **kjs.mof.gov.cn/zhengcefabu/ 支持 index_N.htm 顺序分页**（约 10 条/页，
  index_16≈2017-05~08、index_17≈2016-10~2017-05、index_20≈2014-11~2015-07、
  index_21≈2014-02~07、index_30≈2010-01~07、index_35/36≈2008-05 回填、
  index_37 起 404）。二分定位年代后逐页扫描即可命中历史文件；**不要用搜索引擎**。
- **财政部令在独立档案**：tfs.mof.gov.cn/caizhengbuling/YYYYMM/tYYYYMMDD_N.htm，
  由 kjs 政策发布列表的令条目外链发现；76号令页内直接附基本准则修改后全文。
- **CAS 25 的通知在 gongzuodongtai/ 而非 zhengcefabu/**——kjs 各栏目档案互相
  独立，一个栏目翻到底不等于全部站内没有；跨栏目（工作动态/工作通知）都要查。
- **2006 批（财会〔2006〕3号）确实不在 kjs 档案**（底部 index_36 只有 2008-05
  批量回填的 2003 年文件）——这次是证据充分的 blocked，不是假设。
- **重大教训：把"搜索引擎不可用"误判为"来源不可得"**。第一轮 6 个 blocked
  项（CAS 25/33/37/23、基本准则、增值税会计处理）全部实际可得。结论：对官方
  域名，先穷尽"栏目分页 + 站内树 + 已核验文件中的交叉引用（通知互引文号/日期/
  附件路径）"，仍无才 blocked，且 attempts 必须包含"翻到档案底部"的证据。
- **"HTTP 200 ≠ 内容可得"**：SSE 公告索引 200 但 31KB JS 空壳；flk GET 200
  同样空壳。取证判断必须基于正文内容而非状态码。
- **通知间交叉引用是最可靠的发现通道**：财会〔2017〕7号通知直接给出 8号/9号
  绑定；财会〔2020〕20号给出 2006 批容器与〔2009〕15号废止关系；CAS 37 通知
  给出〔2014〕13/23号。读通知时顺手提取这些坐标。

## 2026-09-10 W3-Task 21：个人交易计划状态机（plans/ 纯模块）

- **模块布局（任务 22/23/24 接 plans 时直接查这里）**：
  - `plans/mod.rs`（235 纯行，warning band）：`PlanPolicy`（反向门槛 2000bp、复核
    阈值 1000/200bp，随 save 固化）、`PlanEvent`（唯一事件入口，接受/成交是不同
    变体）、`PlanBook`（(账户,股票)→PlanId 索引 + 单调 PlanId 序列；serde 只存
    policy/next_plan_seq/plans，索引在 `from_parts` 重建并校验重复/越界 →
    `SaveInconsistent`）、`apply(plan_id, event) -> Result<PlanStatus>` 分发。
  - `plans/state.rs`（142）：全部状态类型 + `TradingPlan::from_open`（三参：
    plan_id, PlanOpen, &PlanPolicy）+ 查询（`last_valid_trading_day`、
    `remaining_share_qty`、`is_terminal`）+ `terminate`。
  - `plans/revision.rs`（248）：事件转移 impl 块（`ensure_event_allowed` 三重守卫
    = 非终态/未越有效期/时间不回拨；observe/acceptance/fill/excess-fill）+
    `apply_revision`（`classify_revision` 纯分类 → Forward/ReverseRestart/
    CompleteNow/Terminate）+ pause/resume/expire/end_of_trading_day。
  - `plans/validation.rs`（241）：`PlanError` 22 变体 + 纯校验 + 公开谓词
    `reverse_crosses_threshold(old,new,S,threshold)`（K5a 迟滞数学，任务 22/23 复用）。
- **K6 语义的关键决定（后续任务不要漂移）**：
  - `Completed` 仅由真实成交到达份额目标触发（fill 或修订到恰好等于已成交）；
    到期/无资金/撤销/超目标成交全部 `Terminated{reason}`。
  - 反向修订跨过门槛后 `filled_qty` 归零（新腿进度），旧腿真实成交留在账户；
    版本+1、清除子单引用。
  - 同向修订目标 < 已成交：必须带 `below_filled_rationale`，否则类型化拒绝；
    带理由则目标如实下调并以该理由终止。
  - 份额目标才有份额级完成/超额语义；`PositionFractionBp` 目标成交照常累计但
    不自动完成（换算属任务 22），excess 路径对其显式拒绝。
  - 有效期语义：`last_valid_day = created + horizon - 1`；日终 `d >= last_valid`
    → 到期终止；事件 `d > last_valid` 拒绝（EventBeyondHorizon）；显式 `Expired`
    仅接受 `d > last_valid`（提前调用 = ExpireBeforeHorizonEnd）。
  - `PlanId` 单调不复用；终止后同账户+股票可开新计划（DuplicateActivePlan 只拦
    非终止计划）。
- **250 行天花板的拆法**：4 文件契约下 state.rs 天然超编（类型 + 事件方法）。
  解法：事件转移 impl 块放 revision.rs（任务规格本来就把 pause/resume/expiry/
  day-end 归 revision.rs —— 它实际是"转移文件"），state.rs 只留类型+构造+查询。
  `ChildOrderLink` 简化为 `active_child_order_id: Option<OrderId>`（对齐
  ParentOrderPlan 字段名）；`ResumeRecord` 简化为 `Option<ResumeReason>`（日期
  已由 last_event_trading_day 覆盖）。
- **lib.rs 共享文件协议实战**：日历 agent 先落了未跟踪 `src/calendar/` +
  `tests/calendar/`，但 `mod calendar;` 尚未进 lib.rs（等我先提交）。我编辑前
  `git diff lib.rs` 确认只含自己的 plans 块 → 快速 check → 只 stage 自己文件 →
  提交，把 lib.rs 让给对方。全量 `cargo test -p engine` 因对方的 calendar 测试
  目标编译失败而 exit 101 —— 用显式 `--lib --test a --test b ...` 枚举跑全部
  16 个非 calendar 套件（478 过/0 败）作为等效证据，并在 failure 证据里如实
  记录排除原因。
- **PowerShell 证据文件头**：PS 5.1 内联 `"`r`n"` 转义在中文环境下解析不稳；
  头部用 write 工具落 ASCII 文件 + `cmd /c copy /b hdr + main combined` 再
  `move /y` 原地替换，比内联拼接可靠。

## 2026-09-10 W3-Task 21 fix pass：守卫先行导致成功路径不可达（重要教训）

- **缺陷模式**：`ensure_event_allowed` 在 `expire()` 的领域条件之前统一拒绝
  `trading_day > last_valid` —— 而 `expire` 的成功条件恰好就是
  `trading_day > last_valid`。两个条件互斥 ⇒ `PlanEvent::Expired` 的成功路径
  完全不可达；错过日终的 `TradingDayEnded` 同样被拒，计划被永久搁浅在 Active。
  编译器查不出（类型都是 Result），测试也没查出——因为测试只覆盖了"提前到期
  被拒"和"日终到期终止"两个分支，**没给每个带成功路径的事件至少一个黄金样**。
- **教训（对所有状态机任务生效）**：事件入口的守卫若与某个事件的语义条件
  重叠，该事件的成功路径可能死亡。验收清单必须逐事件问三件事：
  (1) 它的每条成功路径有没有至少一个 gold test 真正走到？
  (2) 守卫的每个拒绝条件与该事件的领域前提是否相交？
  (3) "补账/迟到"类事件（日终补送、到期扫描）发生在常规时序窗口之外——
  守卫是否为它们留了豁免位？
- **修复形态**（commit `fix(engine): 修复计划到期事件的不可达路径`）：
  `ensure_event_allowed(event, trading_day, allow_beyond_horizon)` 显式布尔参数；
  仅 `expire`/`end_of_trading_day` 传 true（终态守卫与时间回拨守卫仍生效），
  其余 7 个调用点传 false。修复 diff 共 ~10 行、零 API 变更；新增 5 测试
  （可达性×2、新路径防复活×1、守卫仍生效×2），plans 套件 33/33，全量
  `cargo test -p engine` 首次完整绿（485 过/0 败/4 忽略，calendar 已落地）。


## 2026-09-10 W1-Task 4����ʵ�����붳�ύ��������calendar ģ�飩

- **HKO �ٷ�ũ�����ձ��ǿɿ�����ʵ����Դ**��`https://www.hko.gov.hk/en/gts/time/calendar/text/files/T{year}e.txt`
  �����а�� en �� tc��·�� `/tc/`������ʽ��`2030/2/3<2+�ո�>1st Lunar Month<2+�ո�>Sunday<2+�ո�>����`��
  ���³�һ=��1st Lunar Month�������С�����=���³��塢����=����ʮ�塢����=�����С�Bright & Clear����
  1998�C2100 ȫ���ɴԼ 365 ��/�ꡢ~40KB/�꣩��������ץȡ������ 4 ����/�����Ƕ Rust ��������ʱ�����硣
- **HKO 2051 ���ļ�����������ƫ��һ��**��en/tc ���� artifact ͬ����2051-01-01 ʵΪ���ա���ӡ��������
  �ж����ݣ�ȫ�� 37,254 �����ڽ�����֤����һ�겻һ�� + ����ũ����������2050-12-31 ʮ�� �� 2051-01-01 ʮ�ţ�
  + en/tc ũ����һ�¡���ѵ���ٷ���ԴҲҪ���л�����֤������/�����в����þͲ�����ȱ��Ӱ�졣
- **ȡ֤�ű��ĺ����嵥**���ɸ��ã������괺�ڴ��� 1-21..2-21��������Ϊ 4/4-4/5����Ϧ=����إ��/��ʮ��
  �������׼��� 29/30 �졢���ڴ��ڼ�� �� {353..355, 383..385}������ũ����������504 �����׽�����
  ��1998�C1999������ͬһ�ű������������ 504/505 �߽���Ի�Ϊ���ˡ�
- **rustfmt 1.9 �Գ��� derive �б�ǿ������չ��**��money.rs ����ͬ�����ţ���һ�� 11-trait derive ��13 �С�
  ���߼���Ԥ��ʱҪ��������Կ������ȥ��calendar �á��ʻ���������ģ��� + policy/ Ŀ¼���֡�ѹ�� 250 �ڡ�
- **PowerShell 5.1 �� BOM-less UTF-8 Դ���� `-replace` ��д�� = GBK ˫�ر�������**���̻���+���룬
  ���� policy.rs ������д������ƷԴ��������һ���� [IO.File]::ReadAllText/WriteAllText + UTF8Encoding($false)��
  ���� edit ���ߡ������ļ� CRLF/LF ����Ҳ���� Get-Content ����ʧ�桪������������ node ��һ�� \r?\n ��ͳ�ơ�
- **lib.rs �����ļ�Э��ʵ��**������ agent �� `mod plans;` δ�ύʱ��ѯ������Լ 4 ������� 6cdc219����
  ֮��ż� `mod calendar;`���ύǰ `git status --porcelain -- lib.rs` ����ֻ���Լ��ĸĶ���
- serde �Զ��� Deserialize �� `String::deserialize` ������ `<&str>`������ `from_value`��owned��·���ң�
  thiserror Display �ֶ���Ҫ����ʵ�� Display��CivilDate ʵ��Ϊ ISO ������ serde ��ʾһ�£���

## 2026-09-10 W1-Task 6：原子复式记账与总账底座（accounting/）

- **模块布局（任务 7–11 接会计时直接查这里）**：
  - `accounting/mod.rs`（127 纯行）：`Books` = Journal(事实) + Ledger(派生) 组合体；
    `post_batch` 是**唯一过账入口**——结构验证(不变量/重复/封期) → 试算克隆
    Ledger 逐笔 apply（未知科目/NonCash 触现金/溢出在此拦截）→ 批末现金下限 →
    换入。原子性靠「克隆试算 + 通过后 swap」，无需回滚代码。
  - `amount.rs`（223）：`AccountingAmount`(i128 分) + `FractionUnits`(1/10000 分
    的合同累计余数) + 整数半偶舍入 `div_round_half_even`（u128 绝对值，无 f64）。
    serde = 十进制字符串（元.2 位）；`to_money` 检 i64 值域。
  - `journal.rs`（165）+ `journal/state.rs`（100）：分录值类型（pub 字段，非法
    草稿是可表示输入，posting 是唯一验证边界）与 Journal 状态分离——250 行
    天花板的自然切法。
  - `ledger.rs`（190）+ `ledger/chart.rs`（151）：T 型余额 + (科目×期间) 与
    (期间×现金流类别) 增量索引；chart 是版本化 BTreeMap（通用 v1 = 16 个准则
    通用科目，1001/1002 现金类、1602 备抵）。
  - `period.rs`（119）：AccountingPeriod("YYYY-MM" serde) + PeriodStates(封账集合)。
- **K2 关键决定（后续任务不要漂移）**：
  - **事实/派生边界**：Books serde 只存 {chart, journal(batches+closed)}；
    posted 映射与全部 Ledger 索引在 Deserialize 里**重放重建**（逐批走与在线
    post_batch 同一条验证路径 → 篡改存档注入不平衡/重复分录在恢复边界失败，
    有测试锁定）。恢复顺序 = 先全部批次再按期间序封账——因「封期后拒绝一切
    入账」恒成立，该重放与原时序等价。
  - **现金流类别的期问归属**：期初余额凭证计其**所属期间**的 CF（2029-12 的
    Financing 1000），主金样断言 2030-01 经营 120/筹资 500 与计划数字吻合。
  - **批级现金下限**：只有批末状态参与校验（failed_index=None）；批内「先出
    后进」合法——批次是原子单位，中间态不暴露（有测试）。
  - **NegativeCashProhibited 只看 chart 里 is_cash 的科目**；负权益/负净利是
    普通合法状态（equity_rolling 可为负，有测试）。
  - BusinessKind 是纯分类标签（12 个通用变体），不驱动过账逻辑；行业任务
    8–11 直接扩枚举 + 建各自 posting 代码，不动本底座。
- **serde 坑（再现 + 新坑）**：`from_value` 路径 `<&str>::deserialize` 直接炸
  （"expected a borrowed string"），owned `String` 才行——learnings 里任务 4
  已记，这次在 AccountingAmount/FractionUnits/AccountingPeriod 三处统一用
  String。BTreeMap 的**元组键**（(科目,期间)）在 serde_json 下不可序列化——
  Ledger 全部索引刻意不做 serde（派生态不进存档），从根上规避。
- **round-half-even 余数链**：`apply_basis_points_accum` 的不变量
  `Σpaid×10000 + 终余数 == Σ(cents×bp)` 有守恒测试（1000 分@337bp×3 期 →
  34/33/34，余 0.1 分，总和 1,011,000 精确）。RHE 会产生**负的中间余数**
  （34 超付 → 余 -0.3）——合法且守恒，任务 8 的利息子账直接复用。

## 2026-09-10 W1-Task 5：自然日经营时钟（session/civil_clock.rs 双时钟）

- **模块与 API 面**（任务 14/15/28/27 接线时直接查这里）：
  - `session/civil_clock.rs`：`CivilClock`（start/current/settled_through/next_due_seq/
    pending/披露观察者），`CivilClockSave`（存档态，观察者不入档），`DueKind`
    （InterestAccrual/ContractMaturity——**fixture 种类**，真实业务在任务 14）、
    `DueBusiness/DueBusinessId`（时钟分配单调 id）、`CivilPhase`
    （IntradayTrading/ClosedDay；PostClose→Disclosure 是 end_day 内部瞬时窗口）、
    `CivilDayEndReport`（civil 事件面：settled_date/dispatched_due/disclosure_instant/
    next_date/next_status）、`CivilClockError` 8 变体（Duplicate/OutOfOrder/Skipped/
    BeyondRuntimeCeiling/DueRegistrationInPast/MarketSessionOutOfSync/SaveInconsistent/
    透传 Calendar+CivilDate）。`session_calendar_exchange(StockExchange)` 建立
    StockExchange→CalendarExchange 映射（取 setup 首股，政策 v1 沪深同轨）。
  - `GameSession` 新增：`civil_date()/civil_clock()/civil_clock_mut()/end_civil_day()`。
    **step() 完全未动**——civil 推进是收盘后的新接缝，宿主协议（任务 28）负责在
    ticks_per_day 个 step 后调 end_civil_day；现有测试不调它也全绿（空 due 队列下
    跨周末 step 不报错，同步失步要到下一次 end_civil_day 才被 MarketSessionOutOfSync
    抓住——已知妥协，任务 27/28 收紧）。
  - `SessionSetup.start_date`（serde default 2030-01-01=政策 v1 default_start；
    validate 走 `TradingCalendar::default_v1()?.validate_runtime_start`）。
    `SaveSlot.civil_clock` 必填 → 旧档在反序列化边界显式拒绝（K1 不留迁移器）。
  - `SessionError` 新增 `Calendar(#[from])` + `CivilClock(#[from])`；lib.rs 的
    crate-root re-export 列表**尚未**加 civil 类型（lib.rs 归任务 6/后续），当前用
    `engine::session::{CivilClock,...}` 路径访问。
- **日结语义**：end_day(date) 四段校验（重复==settled_through→Duplicate；<current→
  OutOfOrder；>current→SkippedCivilDays 列出被跳 due；next 越过 2099-12-31→
  BeyondRuntimeCeiling）全部在**任何变更之前**；应用段=派发当日 due（恰好一次、
  按 id 序）→18:00 CivilInstant 逐观察者调用→settled_through=current、current=next。
  会话级 `end_civil_day` 先做同步守卫 `session.day == tcount(start..=current 交易日)`
  再进时钟。
- **extraction_replay 摘要变更依据**：事件流 FNV 不变（tick/RNG/撮合零漂移的直接
  证据）；两个存档 FNV 变更仅因 SaveSlot+civil_clock、setup+start_date 字段新增
  （档格式演进至任务 27 定稿）。mid=1_702_442_567_969_422_992，
  end=190_030_750_827_517_148。
- **测试写实的三个坑**：(1) 连续竞价价格笼子——涨停价 1100 买单直接
  PriceCageExceeded（基准 1000→笼 1010），fixture 买价必须在笼内（用 1005）；
  (2) volatility=0 的市场 NPC 零成交（无信号全 Hold），要真实成交需 volatility>0
  +足够 retail；(3) **周末优先于假日标注**：周六除夕是 Closed(Weekend) 不是
  SimulatedHoliday(SpringFestival)，只有节内工作日标假日。场景锚 2030 春节：
  02-01(五)交易、02-02..02-05 休市、02-06(三)复市；2099-12-31=周四（交易日）。
- **ts_rs 12**：`#[ts(optional)]` 只允许 Option 字段（非 Option+serde default 不行），
  start_date 生成端为必填；export_bindings 测试会**改写已跟踪的** generated/*.ts
  （SaveSlot.ts/SessionSetup.ts）——提交前 `git checkout --` 还原，保持该目录
  归任务 29，同时避免 web defaults.ts 因必填新字段编译失败。
- **SessionSetup 字段新增的机械波及清单**（全仓库仅这些字面构造点）：session.rs
  内部 2 处、tests/{session,auction,diagnostics,extraction_replay}.rs 各 1-2 处、
  examples/baseline_fixture.rs 2 处、**apps/server/tests/actor.rs 1 处**（apps 也
  要查！workspace check 才能过）。
- clippy 有一个 behavior/decision.rs:239 unnecessary_filter_map 警告（任务 5 之前
  就存在，behavior/ 不归本任务，未动）。


## 2026-09-11 W1-Task 7：公司实体、经营合同与开局账套（company/）

- **模块布局（任务 8–12/17/26 接公司域时直接查这里）**：
  - src/company/{mod,error,spec,opening,counterparty,contracts,defaults}.rs（7 文件，
    比计划字面 5 文件多 error.rs+defaults.rs——accounting/{mod,error,...} 同款先例；
    defaults.rs 是默认虚构公司数据表）。
  - mod.rs：CompanyConfig（spec+opening+counterparties+budget）/Company（Books+
    对手方登记簿+合同簿+预算+子账占位）/CompanyRegistry（validate_set 先行→逐公司
    装配，任一失败整体不产生）。register_contract 是合同唯一公开入口（数据验证+
    对手方已登记+借款授信占用三重检查；ContractBook::register 是 pub(crate)）。
  - spec.rs：CompanyId/IndustryId/CompanyKind/CompanySpec + validate_set（重复 id、
    重复发行映射、未知母公司、集团环——BTreeSet 沿 parent 链走）。
  - opening.rs：CompanyOpening（chart+as_of+显式行）→ to_voucher()（OpeningBalance
    + Financing 现金流类别，与任务 6 金样一致）；opening_event_id()=1（<2^53）；
    SubsidiaryLedgers = 合同/资产/库存三个占位 unit 结构（任务 8–11 填内容）。
  - defaults.rs：9 家虚构公司（5 上市工商映射默认 5 股票 + 4 未上市测试实体四
    CompanyKind 各一；C-TEST-IND 是 C-600101 的集团子公司）。实收资本=面值1元×
    总股本（与市价无关），issued_shares 与 defaults.ts total_shares 逐字段一致。
- **K2 关键决定（后续任务不要漂移）**：
  - **validate_issuer_mapping 输入是 (StockCode, total_shares) 对**——公司域不
    import session（架构方向：session→company 由任务 26 单向接线）；双向校验：
    UnknownIssuerStock（映射的股票不在清单）/ IssuedSharesMismatch（股本不精确
    相等）/ UnmappedStock（清单股票无发行人）。
  - 开局凭证不设收付流水：counterparty flows 为空（开局是状态不是交易；前史
    现金流由任务 14 生成）。「外部对手方吸收开局资金流」= 开局借款有 Lender
    对手方登记 + 交易账户字节不变（isolation 测试双锚：逐户账户快照 + SaveSlot
    serde 字节对比）。
  - 授信语义：outstanding=Σ已登记 Borrowing 本金；恰好用满（projected==limit）
    合法、超出拒绝；还款核销留任务 8–11。
  - 四种独立测试实体当前全部用 generic v1 科目表（行业科目表任务 8–11 以新
    chart 版本扩充，不改通用表）。
- **PowerShell 写源码再踩坑（第二次）**：Set-Content 对已有 UTF-8 中文文件
  -replace 回写 = GBK 双重编码毁全部中文注释 + UTF8 BOM。修复：write 工具整文件
  重写 + node 字节级剥 BOM（fs.writeFileSync(b.subarray(3))）。**任何源码批量
  改写只用 edit/write 工具，绝不用 PS 字符串管道。**
- tests/company_opening/ 目录形态（main+balanced/isolation/failures/{mod,registry,
  credit,guards}）：QA 命令 `--test company_opening` 不变；15 测试全绿，全量
  cargo test -p engine 561 过/0 败/4 忽略（基线 546+15）。


## 2026-09-11 W3-Task 17：账户身份/主导风格/分析权重解耦（profile 派生契约）

- **模块布局（任务 18–20/22–23/26 接 profile 时直接查这里）**：
  - `src/strategy/analysis_profile.rs`（201 纯行）：`AnalysisWeights`（五 u32 bp 私有字段 +
    `new(i64×5)` 边界校验：负/全0/总和≠10000 → `AnalysisProfileError` 前 3 变体；访问器
    as_array/total_bp/单字段）、`FundamentalMethod`（EarningsMultiple/CashFlow/EquityRoe）、
    `AnalysisProfile`（weights + 方法槽位 Some ⇔ fundamental_bp>0；EquityRoe 进槽位 =
    IllegalMethodKindMapping；`method_for_company_kind(CompanyKind)`：零权重恒 None、
    Bank/Insurance 恒 EquityRoe【规则非抽样】、Industrial/RealEstate 用槽位）、
    `PersistedAnalysisProfile`（i64 + String 方法 id，deny_unknown_fields；
    serde into/try_from 走 DTO → 恢复边界全部类型化校验）。
  - `src/strategy/factory_profiles.rs`（122 纯行）：`default_analysis_weights`（K5 13 风格
    逐字表）、`derive_analysis_profile(&StrategyProfile, AccountId, &mut dyn Rng)`、
    `largest_remainder_normalize([i64;5], u64 target)`（pub，数学契约见 doc）。
- **派生契约（任务 18–20 依赖的关键决定，不要漂移）**：
  - 主导风格是**输入**，derive 绝不重抽；每个**非零**权重按声明顺序（基本面→趋势→量价→
    技术→成本经历）恰好采样一次 0.6–1.4 倍率（整数 bp 6000..=14000，sampling.rs 的
    `[lo, lo+width)` 约定）；**零权重不消耗随机数**（严格 SeqRng 测试锁定 draw 数）。
  - 最大余数法归一回 10000：floor + 按余数降序 +1，**同余按字段声明顺序破同分**；零字段
    恒零是数学保证（deficit < 正余数字段数，+1 到不了余 0 字段），测试含 [1,1,1,0,0]
    tie 金样 [3334,3333,3333,0,0]。
  - 方法选择只在工厂一次：机构 DeepValue/Defensive→盈利倍数、Growth→现金流；**其余一切
    （Balanced/ActiveTrader/散户/游资）按 AccountId 奇偶：偶→盈利倍数、奇→现金流**
    （计划只说“按奇偶”，方向是本任务定的文档化选择）；仅 fundamental_bp>0 时生效。
  - **会话接线警告（任务 26）**：derive 接受注入 `&mut dyn Rng`；populate_npcs 里必须用
    **独立种子**的 SplitMix64（如 seed ^ account 派生常量），绝不能直接用 self.rng 内联
    抽样——否则 session RNG 流移位，extraction_replay 字节锚点会红。
  - 三个手算金样（固定 f64 序列→倍率→归一）在 tests/analysis_profiles/gold.rs：
    LongTerm [3448,1609,575,920,3448]（deficit=2）、ActiveTrader [0,3418,2532,2531,1519]
    （零权重不抽）、Reversal [0,900,2500,4200,2400]（整除 deficit=0）。
- **ts_rs 未加**：profile 目前是 engine 内部层，web 消费在任务 22–23/26/29；届时再补
  TS derive（新 .ts 文件天然 untracked，无冲突）。
- 全量 `cargo test -p engine` 585 过/0 败/4 忽略（基线 561+24 新增）。

## 2026-09-11 W2-Task 8：工商经营与营运资金会计（accounting 共享子账 + company/industrial）

- **模块布局（任务 9–14 接行业会计时直接查这里）**：
  - `accounting/{inventory,fixed_assets,receivables,tax}.rs`（四份**行业中立共享子账**，
    accounting/mod.rs 私有 mod + pub use 再导出）：inventory=数量+成本+科目绑定
    （receipt/preview_issue/apply_issue，移动加权 rhe(结存×量/结存量) 结构性守恒）；
    fixed_assets=登记+直线折旧（rhe(剩余基础/剩余月数) 守恒）+减值面（上限=账面−残值）；
    receivables=`TradeOpenLedger` 应收/应付同构开项（部分核销/账龄 4 桶/逾期/核销）
    + `ecl_allowance_target`（CAS 22 §63 整个存续期简化法）；tax=`TaxPolicy`
    （**无默认构造器**，vat-law/cit-law 仍 blocked）+ 价外税销项/进项拆分
    （`split_input_vat`，不可抵扣归集进成本）+ `compute_income_tax`（亏损 FIFO
    弥补 + 到期出池 + DTA=未用亏损×税率**全额确认简化**，纯函数返回 ending_pool）。
  - `company/industrial/`（12 文件，超计划字面 7 文件——250 行天花板所致，见 issues）：
    mod（IndustrialBooks 结构/装配/accessor/post_with_commit）+ config（配置类型 +
    开局种子对账守卫）+ error + chart（工业科目表 v2 数据表 + acct 常量）+ loans
    （LoanState + ACT/365F 数学）+ interest（借入/计提）+ repayment（付息/还本）+
    purchasing/production/sales/expenses/capex。
- **共享子账复用契约（任务 9–11 必读）**：子账只管数量/成本/开项事实，**过账一律由
  行业处理器做**；处理器不变量 = validate（子账 peek 预检）→ `post_with_commit`
  （原子过账，成功才推进 next_event_id）→ apply（子账落地）。任何拒绝（含
  PaymentFailed）连事件 id 都不消耗 → `assert_eq!(co, before)` 字节不变可断言。
  VAT/坏账准备/DTA 的已入账余额**直接读总账科目**（net_of），不设影子计数器。
- **开局借款治理决策（O2 裁定，任务 9–11/14 沿用）**：开局 2001 余额 = 隐式合同
  `OPENING-DEBT`（构造时与 2001 贷方余额精确对账，不匹配→OpeningDebtMismatch），
  与普通合同共用计息/付息/还本/授信占用（容量=限额−Σ未偿含开局）。**开局隐式
  合同的过账科目恒为 2001**（loan_account_of 特判——期限推导会错选 2501，已修）。
- **ACT/365F 余数约定**：`FractionUnits` 承载合同累计余数，单位 = 1/3_650_000 分
  （10_000bp×365 的自然小数单位；与任务 6 apply_basis_points_accum 的 1/10000 分
  单位不同——单位语义由调用点定义并在 loans.rs 文档钉死）。银行（任务 9）计息直接
  复用 `loans::accrue_act_365f`。
- **rhe_div 三副本**（amount.rs 私有原件不可动）：accounting::inventory（共享给
  fixed_assets，pub(in crate::accounting)）+ industrial::loans（crate 内私有）。
  均为同算法孪生、带交叉引用注释；任务 13 如需统一入口再议。
- **BusinessKind 粗标签债**：journal.rs（任务 6 语义冻结区，本轮禁改）无行业变体，
  工业事件以最接近通用标签记录（赊购=CreditSale/NonCash、生产结转=Depreciation/
  NonCash、核销=ReceivableCollection/NonCash 等，完整映射表在各处理器头注）。
  **任务 9–11 或 13 需扩枚举**（journal.rs 自己的文档预留了此步），届时可迁移标签。
- **已知边界（简化登记）**：生产=单事件领料+完工（无多日在产品/制造费用科目）；
  费用现付（无应付职工薪酬循环）；折旧全归 6602；减值后不重估剩余寿命；
  DTA 全额确认；进项富余只留抵不退税；利润表口径亏损弥补为年度（year_pretax
  按 1–12 月期间索引求和）。
- **测试金样对账锚（chain_gold）**：终态现金 20451.21/净利 3991.57/净负债
  9859.64/权益滚动 13991.57（资产−净负债=权益成立）；还本后现金 12801.21/
  权益 13985.43；利息 31 天=679 分（余 1,650,000 单位）——手算钉死，防漂移。


## 2026-09-11 W3-Task 19：具名技术指标内核 + 个人价格记忆（observation/experience 接缝）

- **模块布局（任务 20/22/23/25/26 接线时直接查这里）**：
  - `src/strategy/technical.rs`（173 纯行）：纯数学内核。`sma(&[Money], window)`、
    `rsi14(&[Money])`、`atr14(&[TechnicalDailyBar])` + 4 窗口常量
    （SMA_SHORT/LONG_WINDOW=20/60、RSI_WINDOW=ATR_WINDOW=14）。`TechnicalError`：
    InsufficientHistory{available,required}（携带可用长度）/ NonPositivePrice{index,cents}
    / InvertedRange / ZeroWindow。**输入校验先于长度检查**（脏价格优先报
    NonPositivePrice，即使样本也不够——测试锁定了该顺序）。
  - `src/observation/technical.rs`（72 纯行）：`TechnicalDailyInput{trading_day,high,low,
    close,volume}` + `build_technical_observation(&[TechnicalDailyInput], as_of_trading_day)
    -> Result<TechnicalObservation, ObservationError>`。`TechnicalObservation` 四字段各自
    `Result<指标, TechnicalError>` + valid_sample_count——独立可用、互不耦合（短期跌/
    长期涨同时表达）。结构校验（ObservationError）：日序严格递增+连续（复用
    NonIncreasingDay/TradingDayGap 语义）+ 新变体 DailyBarNotBeforeObservation{day,as_of}
    （当日未完成 K 与未来 K 拒绝）。
  - `src/experience/price_memory.rs`（112 纯行）：`PersonalPriceMemory{stocks: BTreeMap}`
    + `StockPriceMemory`（first/last_observed_minute+price、observed_high/low、
    last_public_history_read_minute、public_history_read_count、last_touched_minute；
    serde u64_decimal 字符串 + ts_rs 已生成未跟踪）。方法：`observe_price`（本人所见，
    建立锚点+累计高低）、`record_public_history_read`（记录读取事件，不动锚点/高低，
    刷新 last_touched；未观察过的股票 UnobservedStock 拒绝）、`prune(&BTreeSet)`
    （protected=持仓∪活跃计划由调用方组合——**任务 25 接计划链接的文档化接缝**）、
    `stock(code)`/`stock_count()`（任务 22 记忆查询面）。
- **关键数学决定（任务 22 K5a 聚合直接消费，不要漂移）**：
  - RSI 采用**简单平均（Cutler 式）而非 Wilder 递推**：日 K 有保留上限（360/250 日），
    Wilder 需从序列起点连续平滑，截断窗口下不可确定性重建；简单平均无状态重算、
    存档恢复前后逐位一致。RSI14 = round_half_even(100·Σgain/(Σgain+Σloss))（/14 约去），
    输出 u16 0..=100；**全平（分母 0）且 14 个有效样本 = 精确 50 + all_flat=true**；
    需要 15 个收盘（14 个涨跌样本）。
  - ATR14 = 最近 14 个 TR 的简单平均；**每个 TR 需要前收盘 → 需要 15 根日 K**（不虚构
    首根前收盘）。TR=max(H−L, |H−PC|, |L−PC|)。K5：ATR 只供风险/执行，绝不冒充方向
    （类型文档已钉死，任务 22 聚合时不得把它接进方向分）。
  - 一切除法半偶舍入（strategy/technical.rs 的 div_round_half_even 是 accounting::amount
    同算法第三副本，与 industrial::loans 先例一致）。
  - **无成交日（volume==0）不构成指标样本**：K5「真实已发生行情」——先按原始连续日序
    校验（缺日=TradingDayGap），再排除 volume==0 的日子；全无成交 → 各指标
    InsufficientHistory{available:0}，不补零。预置历史 volume>0 照常参与（与既有
    5日/20日窗口口径一致）。
- **Rust 2018 模块布局**：`src/observation.rs` + `src/observation/technical.rs` 并存
  （experience.rs 同理）——**无需把单文件改造成 mod.rs 目录**，公共路径全部原样保留，
  lib.rs 零改动（任务指令 staging 清单不含 lib.rs）。给单文件模块加子模块直接用这个布局。
- **ObservationError 不能 derive PartialEq**（透传 MoneyError 无 PartialEq/Eq；money.rs
  不在本任务范围）→ 测试用 `matches!(err, Variant{字段绑定})` 断言，等价精确。
- **会话接线归任务 26**：本任务只交付纯观测接缝（build_technical_observation），未触碰
  session/views.rs（MUST NOT 列表禁改 session*）。session 侧把 daily_candles（含过滤
  volume==0）转 TechnicalDailyInput 的装配在 26 的决策链接线里做。
- 全量 cargo test -p engine 首次完整绿：660 过/0 败/4 忽略（含本任务 technical_memory
  33/33 + 并发任务 9 的 bank_accounting 11 过）。


## 2026-09-11 W2-Task 9：银行经营会计与报表分类（company/bank + accounting/reports）

- **模块布局（任务 10–14 接行业会计时直接查这里）**：
  - `company/bank/`（11 文件）：mod（BankBooks 结构/装配/accessor/post_with_commit/
    presentation_lines 胶水/ensure_counterparty+record_flow）+ chart（v3 科目表，代码
    真源 = accounting::reports::bank::codes）+ config（BankConfig + 开局子账种子守卫）
    + error + deposits（DepositState + 存/取）+ loans（BankLoanState + ACT/365F 数学
    孪生 + validate_terms）+ lending（发放/收本）+ interest（**全部计息结息**：贷款
    应计/收妥 + 存款计提/支付）+ ecl（EclStage/EclScenario/EclPolicy/阶段转移/
    assess_credit）+ writeoff（核销/回收）+ fees（手续费）。
  - `accounting/reports/`（NEW）：mod + bank.rs——**报表分类层**（非完整报表，
    任务 13 消费）。`bank_presentation_lines(&Ledger) -> BankPresentationLines`：
    利息净收入/手续费及佣金/信用减值损失/贷款总额-准备-净额/客户存款/现金头寸/
    应付利息。**任务 10/11 的保险/地产列报以同构方式加 reports/{insurance,
    real_estate}.rs**；行业科目代码真源放 reports 侧（company→accounting 单向依赖
    不允许反向 import），company 侧 chart 构造器引用同一 codes 防两层漂移。
- **K3 关键决定（任务 10–14 不要漂移）**：
  - **ECL 除数 = 10^12**：准备目标 = rhe(Σ(权重×PD×LGD×账面)/10^12)——三个基点
    因子各 10^4。第一版写成 10^8 被金样抓住（准备大 10^4 倍）。EAD=账面余额
    （本金+应计利息）；概率加权 = 情景列表 Σ权重必须恰为 10000bp（构造期与逐次
    重估双守卫）；**不折现**（无贴现参数即不虚构贴现假设，简化登记 issues）。
  - **阶段 3 净额法计息**：基数 = max(账面余额−准备, 0)（准备可临时超账面——
    回收后未重估的窗口；显式零下限规则）。阶段 1/2 毛额（本金）。
  - **现金流分类选择**：银行对客存/贷/收息/付息/手续费全部记 **Operating**
    （CAS 30 (2026) §45–§47「向客户提供融资为主要业务活动」归类选择，游戏
    固定选经营——与工商业借款记 Financing 不同，勿混）。
  - **核销前置**：InsufficientAllowance 守卫要求准备 ≥ 账面（先 assess 100%
    情景）；回收 Dr 现金/Cr 1303（准备贷记），回收后重估（同阶段允许）转回
    超额准备——已核销贷款**改阶段**被拒、同阶段重估合法。
  - **存款到期停息**：accrue 的有效截止 = min(计提日, 到期日)，last_accrual
    推进到**有效截止**而非计提日（否则下次计提误报 AccrualNotForward——修过
    的真实 bug）。提前提取按原利率（无活期/定期切换，简化登记）。
  - ACT/365F 复用冲突：learnings 曾写「任务 9 直接复用 industrial::loans::
    accrue_act_365f」，但该函数 pub(super) 私有且本任务禁改 industrial → 按
    rhe_div 孪生先例（第四副本）复制 + 交叉引用注释。
- **金样锚（chain gold，分）**：存 1000.00/费 5.00/贷 600@600bp；27d 贷息
  266（余 1,100,000）/30d 存息 123（余 1,050,000）；阶段 2 目标 2,411（补 2,111）、
  阶段 3 目标 2,400（转回 11）；净额 28d 计息 265（余 1,530,000）；100% 计提
  60,265；核销 60,265；回收 10,000 + 重估转回 10,000。终态：现金 250,528/
  NI −49,472（=531−238+500−50,265）/权益滚动 150,528 = 资产−存款 ✓。
  **教训：手算 3,650,000×266 曾误作 969,900,000（实为 970,900,000）**——乘法
  逐位复核再写进断言；金样先跑实现再对注释，两处都要改。
- 测试目录形态：tests/bank_accounting/{main,gold}.rs + failures/{mod,guards,ecl,
  contracts}.rs（任务 8 目录先例）；gold.rs 284 纯行（单场景例外登记，同任务 8
  chain_gold 298）。全量 cargo test -p engine 660 过/0 败/4 忽略（含并行任务
  新增套件）；clippy 仅剩 behavior/decision.rs:239 既有缺陷。


## 2026-09-11 W3-Task 20：真实经历接入信心、忍耐与风险压力（experience/feedback）

- **模块布局（任务 22/23/25/26/27 接经历反馈时直接查这里）**：
  - `src/experience.rs`（309 纯行，warning band+mandated additive）：`RetailExperienceState`
    新增 `#[serde(default, skip_serializing_if = "ExperienceFeedback::is_empty")] pub feedback`
    字段——**默认空反馈不序列化**，旧档字节与 extraction_replay 锚点完全不变；
    `ExperienceError` 追加 7 变体（三时钟回拨/未来经历/两个生命周期丢失/
    InconsistentFeedback）。构造器 `new`/`without_equity_reference` 显式填默认。
  - `experience/feedback.rs`（144 纯行）：`ExperienceMoment{civil_date, market_minute,
    trading_day}`（三时钟分量各自单调，K1 互不换算）、`FailureEventRecord`（code +
    order_id + 确认时刻）、`HoldingEpoch{entry_moment, last_own_observation}`、
    `ExitRecord{cooldown_until, realized_profit, moment}`、`ExperienceFeedback{latest_moment,
    failure_events, stocks, exit_records}` + 守卫（`ensure_moment_forward` 纯检查 /
    `advance_clocks` 提交段推进 / `ensure_as_of_reached` 读取守卫）+
    `validate()`（任务 27 恢复边界调用）。
  - `experience/feedback/lifecycle.rs`（145）：`RetailExperienceState::{initialize_holding_dated,
    record_fill_dated, observe_position_dated}` 三段式（纯守卫→legacy 原样执行→提交段）。
  - `experience/feedback/inputs.rs`（37）：读取输入 `failure_influence(as_of)` /
    `is_long_stuck(code, cost, as_of)` / `experience_drawdown_from_peak(equity)`。
  - `tests/experience_feedback/{main,seam,reads}.rs + failures/{mod,clocks}.rs`（25 测试）。
- **K5 关键决定（任务 22/23 聚合时不要漂移）**：
  - **事实与派生分离**：状态只存事实（受挫登记/生命周期/退出历史）；忍耐/信心/
    风险压力档位是**读取时派生**的输入，不存第二份会漂移的副本。失败影响 =
    `consecutive_failed_buys`（复用 legacy 计数，含获利退出恢复语义）−
    `(as_of_day − 最近受挫day)/20` 档，饱和于 0；**衰减永不改计数与登记**。
  - **失败确认严格镜像 legacy 增点**：observe（≤买入价 95%）与亏损卖出（首笔卖单
    + `!adverse_move_recorded`）两处，在 legacy 调用**前**预判、成功后登记；
    成交本身绝不直接产生失败事件（未成交/被撤订单没有任何写入路径）。
  - **生命周期总规则**：dated 成交里，从零建仓要求无活跃 epoch；加仓/任何卖出
    要求有活跃 epoch（违反=丢失生命周期，类型化拒绝）。清仓把 `cooldown_until`
    从 legacy 字段读回（不二次计算/二次溢出路径），epoch 移除 + 退出历史追加，
    **再入场不抹历史**。
  - **长期被套**：`持有 ≥20 交易日 && 最近本人观察价(含成交价) < 读取方传入的
    当前权威成本`——成本不复制进经历状态（K5 复用状态）。
  - **决策体零改动**：`behavior/decision.rs` 冻结函数体未触碰；同损益不同经历的
    分叉经既有 `decide_retail_position_with_experience` 读缝（consecutive≥2 →
    LowConfidence 降级）验收；衰减/被套档位由任务 22/23 在聚合时消费。
- **共享工作树实践**：并发 real_estate agent 的 fmt 偏差让 `cargo fmt --all --check`
  全红——**只对自己文件跑 `rustfmt --edition 2024 <files>`**（绝不在共享树
  `fmt --all` 写回）。全量 `cargo test -p engine` 在对方中途态也能跑（exit 0）。
- ts_rs：`RetailExperienceState.ts`（已跟踪）被测试运行改写 → `git checkout --`
  还原（diff 仅 feedback 可选字段 + import）；新增 ExperienceFeedback/ExperienceMoment/
  FailureEventRecord/HoldingEpoch/OwnObservation/ExitRecord .ts 未跟踪，归任务 29。
- 全量 `cargo test -p engine` exit 0（experience 7/7 原样 + experience_feedback 25/25
  新增 + 其余套件全绿，含并行 real_estate 20 过）。


## 2026-09-11 W2-Task 11：地产开发预售与交付会计（company/real_estate + reports/real_estate）

- **模块布局（任务 12–14 接地产会计时直接查这里）**：
  - `company/real_estate/`（13 文件）：mod（RealEstateBooks 结构/装配/accessor/
    post_with_commit/presentation_lines 胶水/勾稽入口 development_inventory_total）
    + chart（v5 科目表，代码真源 = reports::real_estate::codes）+ config
    （RealEstateConfig + CapitalizationPolicy + 开局种子守卫）+ error + projects
    （ProjectState 成本轨迹 + 资本化窗口逐日判定）+ land/development/presales/
    delivery/borrowing_costs/debt_service/impairment + loans（ProjectLoanState +
    ACT/365F 双余数链数学孪生）。
  - `accounting/reports/real_estate.rs`：列报分类层（开发存货毛/净额、合同负债、
    应收尾款、营业收入/成本、财务费用、减值损失）——**任务 13 报表生成器消费**；
    行业 codes 真源模式与 bank 相同（company→accounting 单向）。
- **资本化政策（任务 13/14 的固定口径，勿漂移）——版本化游戏假设**
  （CAS 17 两轮 kjs 取证 + 本轮 tfs 令/档通道一次补证均不可得）：
  - 窗口 = [首次开发投入日, 完工日)：**首笔开发成本开启窗口**（土地款单独不触发）、
    完工日（含）起永久终止（此后利息必须进 6603，测试锁「不无限资本化」红线）。
  - 中断：闭合区间长度 ≥ suspension_min_days（Fixture 默认 90）→ 区间内费用化；
    < 阈值 → 照常资本化；**开放中断**在计提时已历时 ≥ 阈值 → 本次计提的中断日
    全部费用化。早前计提已资本化的天数**不追溯重述**（确定性声明，登记 issues）。
  - 计提 = 资本化/费用化**两条独立 FractionUnits 余数链**（各自守恒
    Σpaid×3_650_000 + 终余数 == Σ(本金×bp×链内天数)；两条链共用 2231 应付利息）。
  - 借款指定项目（borrow 的 Option<ProjectId>）；未指定项目恒费用化。
- **K3 地产红线锁定**：预售收款 Dr 现金/Cr 2203 合同负债（CAS 14 §39 已核验），
  金样断言 6001 恒 0；交付 = 控制权转移（§4/§13）单张分录 Dr 2203+1122+6401 /
  Cr 6001+1541；尾款只清应收不重复计收入；交付前完工是硬前提
  （DeliveryBeforeCompletion 类型化拒绝）。
- **现金流分类选择（文档化）**：购地/开发投入记 **Operating**（开发存货是
  开发商品的原材料存货，非固定资产投资；CAS 31「投资筹资之外即经营」归类）；
  借款/付息/还本记 Financing——与工商 capex=Investing 不同，勿混。
- **开发成本计价**：项目子账自带「套数×rhe(结存成本/结存套数)」MWA 公式孪生
  （InventoryLedger 数据面不支持只加成本不加数量，故不能直接复用其结构，
  公式与 rhe_div 孪生先例一致，第六份 rhe 副本登记 issues）；守恒
  Σ结转 + 期末结存 == 土地+开发+资本化利息，金样逐位锁定。
- **金样锚（分）**：本金 1,000,000 @730bp ⇒ 每日恰 200 分无余数；资本化
  17,800+3,200+200 = 21,200、费用化 18,200+3,000+6,200 = 27,400（合计 243d×200）；
  项目成本 2,021,200、交付 6/10 套结转 1,212,720、余 808,480；减值 108,480；
  终态现金 3,951,400、NI 1,651,400、资产 = 权益 = 4,651,400。
  **教训：金样里 land→borrow 顺序写错导致中间现金断言 28,000/38,000 全错**
  （终态反而对）——中间锚要与终态锚一起手算复核。
- **shared-file 协议与并发**：本轮 journal.rs/reports mod/company mod 三处 additive
  编辑时无对手未提交行；task-20（experience feedback）的未提交 diff 使
  extraction_replay 存档锚漂移（RetailExperienceState 新增序列化字段）——
  以 21 个显式枚举套件全绿（639/0/4）作等效证据如实登记，锚更新归 task-20。
  **rustfmt 直接调用必须 --edition 2021**（workspace edition；用 2024 会得到
  不同的 import 排序，cargo fmt --check 仍红）。另：cargo fmt --check 在 HEAD
  3bb5096 对 bank/industrial/defaults 本就不绿（工具链版本差异，非本任务引入）。
- 全量 639 过/0 败/4 忽略（21 套件显式枚举，extraction_replay 单测因并发
  task-20 未提交改动漂移，见 issues）；real_estate_accounting 20/20。

## 2026-09-11 W2-Task 10：保险经营会计与合同负债报表（company/insurance + reports/insurance）

- **模块布局（任务 12–14 接行业会计时直接查这里）**：
  - `company/insurance/`（10 文件）：mod（InsuranceBooks 结构/装配/accessor/
    post_with_commit/ensure_counterparty+record_flow/presentation_lines 胶水/line()）
    + csm（计量数学：rhe_div 孪生第五副本、simple_discount_pv、unit_release 守恒分摊）
    + groups（ContractGroupState：GMM 组件 + 责任单元进度 + 赔案子账 + 调节表累计量
    accessor）+ premium（建立组+保费收讫）+ service_release（责任单元释放）+ claims
    （ClaimId/ClaimState + 发生/支付）+ remeasure（重估三向分流）+ chart（v4，代码真源
    = accounting::reports::insurance::codes）+ config（InsuranceConfig + DiscountAssumption
    + 开局种子守卫）+ error。
  - `accounting/reports/insurance.rs`：insurance_presentation_lines(&Ledger) →
    保险服务收入/费用/业绩 + 保险财务损益 + LRC/LIC + 应收保费 + 现金头寸
    （CAS 25 §84/85 + CAS 30 §55(二)）。
- **GMM 游戏模型核心决定（任务 13 报表/任务 14 前史不要漂移）**：
  - 账面 LRC 恒等式：盈利组 `LRC = E_rem + RA_rem + CSM − F_rem`；亏损组（CSM=0）
    `LRC = E_rem + RA_rem − F_rem`——亏损成分是**备查组合成分，从不加余额**；
    期满全部组件精确清零（末批 sweep + 逐组件 FractionUnits 余数链守恒）。
  - 贴现：单利整数 `PV = rhe(claims×3_650_000/(3_650_000+rate_bp×days))`（400bp×365d
    分母 = 3_796_000，恰 = 800/1.04）；F（财务损益总额）= claims − PV，随责任单元回拨。
    RA 不折现、保费挂应收不折现（登记简化）。
  - 重估三向分流（§29(b)/§33/§34/§46–§49）：ΔPV 由 CSM 吸收（**不过账**，组合成分）；
    ΔF = ΔE−ΔPV 立即过 6541（贴现差分量不递延）；CSM 耗尽→亏损成分即期 6451、
    亏损组转回全额冲 6451 后盈余转 CSM。释放/重估事件各最多两张分录（收入/财务、
    财务/亏损），一个事件原子 post_batch，事件 id 按最大槽位推进（零过账也消耗，
    ecl.rs 同语义）。
  - 收入释放含预期赔付+RA+CSM 分量但**不含亏损成分循环摊销**（净损益与 IFRS 17
    循环摊销等价，列报口径简化——issues 登记）；实际赔付走 6451 费用独立于释放。
- **金样锚（分）**：盈利组 premium 1000/claims 800/RA 50/400bp/365d → PV=76,923、
  CSM₀=18,077、F=3,077、全量释放收入 103,077、净利 20,000=1000−800；亏损组
  premium 600 → 首日亏损 21,923、LRC 81,923、净利 −20,000。重估锚：首段 100 单元后
  E_rem 58,082/RA 3,630/CSM 13,124/F 2,234/LRC 72,602；ΔE+8,000（265d 分母 3,756,000）
  → ΔPV 7,774 吸收、ΔF 226 过账；ΔE+50,000 → ΔPV 48,589、亏损 35,465；ΔE−30,000
  → ΔPV −29,153、转回 15,917 + CSM 13,236。调节表五恒等式（LRC 滚动/LIC/CSM/现金/
  损益经济恒等式）在 claims.rs 金样逐条精确断言。
- **教训（任务 9 同款再现，两次）**：400×365 误乘成 1,460,000（用 3650）→ 分母写成
  5,110,000（正确 3,796,000）；18,077×100/365 的 floor 商算错一位。两处都是实现正确、
  **测试期望值错**——金样复核时用「反向验证法」（800/1.04 = 769.23 恰与整数除法吻合）
  比逐位长除法更可靠。
- 测试目录形态：tests/insurance_accounting/{main,gold,remeasure,claims}.rs +
  failures/{mod,guards,entities,unsupported}.rs；17 测试全绿（10 拒绝路径全部
  assert_eq!(ins, before) 字节不变）。全量 cargo test -p engine 728 过/0 败/4 忽略
  （基线 711+17）；本任务新代码 clippy 0 警告、fmt 干净。

## 2026-09-11 W2-Task 12：固定集团合并与抵销（accounting/consolidation）

- **模块布局（任务 13/26 接报表与会话时直接查这里）**：
  - `accounting/consolidation/`（8 文件）：mod（`consolidate(ConsolidationRequest)`
    → `ConsolidationOutput` 流水线 + 输出契约 + 再导出）、group（`MemberId`/
    `ScopeId`/`MemberSpec` 镜像/单层集团校验 → `ValidatedGroup` +
    `SubsidiaryOwnership` 精确基点）、aggregate（期间覆盖一致 + 跨科目表按代码
    加总 + 同代码语义冲突检测）、eliminate（往来配对去重 + `apply_worksheet` +
    共享 helper ensure_member/member_def）、sale（销售申报校验 + 未实现利润 +
    抵销分录）、worksheet（申报/底稿值类型）、minority（调整后成员经济量 +
    少数拆分）、error（`ConsolidationError` 22 变体 + `DeclaredSide`）。
  - accounting/mod.rs：`pub mod consolidation;` + 17 符号再导出（company 不得被
    accounting import——集团输入是 task-7 CompanySpec 的**会计域镜像**，由调用方
    派生）。
- **任务 13 消费的输出契约（不要漂移）**：
  - `ConsolidationOutput`：scope（ScopeId::Consolidated(母 id)）、members（id 序）、
    adjusted_balances（BTreeMap<code, ConsolidatedBalance{def, D/C 合计, net_debit}>，
    仅含有发生额科目）、worksheet（申报顺序）、minority（Vec<MinorityInterest>：
    bp + 子公司调整后 NI/权益 + 少数份额）、六总量（合并/归母/少数 × 损益/权益）
    + consolidated_cash（== Σ 成员现金，逐分）。
  - 调整后口径 = 总账 + 工作底稿行效应；`WorksheetLine` 带 member 归属——上游
    未实现利润冲减子公司损益 → 少数按比例自然分担（少数份额用调整后 NI × bp，
    归母 = 合并总量 − Σ 少数，减法精确无守恒缺口）。
  - 抵销分录**永不回记账套**：现金红线由申报科目非现金守卫 + 抵销科目天然非现金
    双保险（IntercompanyTouchesCash 类型化拒绝）。
- **关键语义决定**：
  - 持股 bp = held×10000/issued **整除精确**（不整除 → OwnershipNotRepresentable，
    绝不静默舍入）；控制一致性 = bp 严格 > 5000（恰好半数拒绝）。
  - 期间覆盖 = 成员**有分录的期间集合**精确相等（journal entries 派生）；休眠
    成员缺记账即 PeriodCoverageMismatch（列表两侧集合）。
  - 往来配对按**成员对**去重：资产侧 + 负债侧各申报一次是正常形态，只生成一条
    抵销（首版 bug：两侧各生成一条 → AR 双倍抵销，金样抓出）。
  - 未实现利润 = rhe((P−C)×U/P)；销售抵销 = Dr 卖方收入 P / Cr 卖方成本 P−未实现
    / Cr 买方存货 未实现（为零省行）。
  - NI 行效应符号：NI = (−Σ收入净借) − (Σ费用净借) ⇒ **收入与费用行的 ΔNI 都是
    −Δ净借**（费用行写成 + 是首版 bug，金样抓出：子公司调整后 NI 偏差 2×未实现）。
  - 跨科目表聚合按代码：同代码要素/现金/备抵必须一致（名称可不同）；只存在于
    一侧的代码只由该侧贡献（1002 工业 / 1003 银行并存合法）。
- **金样锚（分）**：混合 80%（工业母+银行子）：NI 800,000（少数 40,000/归母
  760,000）、权益 10,800,000（少数 440,000/归母 10,360,000）、现金 7,800,000。
  上游/下游（P=1,000,000/C=600,000/U=500,000 → 未实现 200,000）：上游少数损益
  40,000 / 权益 240,000；下游少数损益 140,000 / 权益 340,000（对照断言）。
  全售出：底稿只两行、1405 合并 = 0、COGS = 600,000（集团口径）。
- **clippy result_large_err**：错误枚举大变体（CounterpartyMismatch 两侧 DeclaredSide
  ≈136B）会在**每个**返回该错误的函数上爆 lint——把 side_b 装 Box（错误路径非
  热路径）即可全消；测试侧 `side_b.amount` 自动 Deref 不用改。
- tests/consolidation/ 目录形态：main（夹具）+ gold（混合 80% + 全售出）+
  intercompany_gold（上游/下游）+ failures/{mod,intercompany}；23 测试。金样注释
  写「元」、执行值「分」，与 tests/accounting 先例一致。
- 共享树两次被 task-14 中间态阻断（company/operations 编译错误；其提交后自愈）
  ——29 套件显式枚举（752 过/0 败/4 忽略）作等效证据；company_operations 其
  未完成态 5 failures 属其范围。


## 2026-09-11 W2-Task 14：自然日经营演化、到期调度与共同经济事件（company/{rng,events,scheduler,operations}）

- **模块布局（任务 15/26 接线时直接查这里）**：
  - `company/rng.rs`（69 纯行）：`OperatingRng`（SplitMix64 孪生、serde 状态）+
    `RngStream{CompanyOperating,MarketShock,IndustryShock,InitHistory}` 派生
    （seed ^ FNV1a(tag+id) → SplitMix64 终结器）。
  - `company/events.rs`（179）：`ShockParams`（default_v1=K4 100/100/200bp、5-30d、
    ±500/±1000/±2000bp；stress_v1 独立显式参数）+ `ShockKind` 八变体目录 +
    三个采样入口（幅度=符号位×[1,band]，杜绝零幅度）。
  - `company/scheduler.rs`（181）：`OperatingScheduler` 持久化队列，稳定排序
    (due_date,id)；业务 key 去重（弹出后可复用，惯例含日期）；恢复走
    from_parts 全量校验。**股东动作只存在于 `SchedulerRequest::ShareholderDistribution`
    输入面，提交即类型化拒绝（K3 红线）**。
  - `company/operations.rs`（28，薄入口）+ `operations/`12 文件：
    config（IndustryBooks/FlowParams 枚举 + 装配守卫）/ core（装配+访问面）/
    day（advance_civil_day 五段循环+行业分派器）/ dispatch（due→处理器路由：
    `AR:`回款、`LN:`收本收息、`DL:`交付）/ injections（结构化冲击注入+滚动利息）/
    state（活跃冲击聚合）/ error / history（generate_history）/ industrial /
    bank / insurance / real_estate 流。
  - `session/company_operations.rs`（66）：`CompanyOperationsClockWiring`
    （mirrored id 集合，serde）——scheduler 待办镜像为时钟 DueKind（利息→
    InterestAccrual、合同→ContractMaturity），run_day_end 在 end_civil_day 报告
    后推进经营并增量再同步。任务 26 接宿主循环；18:00 披露归任务 15 观察者。
- **调度器/前史契约（任务 15/26 不要漂移）**：
  - advance_civil_day 严格逐日（DateOutOfSequence）；日内顺序 = 到期派发 →
    冲击到期/采样（市场→行业 IndustryId 序→公司 CompanyId 序）→ 行业流
    （CompanyId 序）→ 次日滚动利息排队（key `INT:{co}:{iso}`，id 恒为当日最大
    → 次日派发序 = 到期在前、利息在后）。
  - 流提交的到期引用语法：工商 `AR:AR-{event}`（赊销 5 天期）、银行
    `LN:LN-{seq}`（到期收息收本）、地产 `DL:PS-{seq}`（完工+lag 交付）。
    前缀未知 → UnknownMaturityReference 类型化错误。
  - 前史：`generate_history(config, start)` = [start.year-2 的 1 月 1 日,
    start 前日]；账套 as_of 必须由调用方设为前史首日前一天（行业账套不暴露
    as_of，带借款账套靠首次计息 AccrualNotForward 兜底，无借款账套是调用方
    契约——issues 登记）。init RNG 流仅前史期间使用，完成后 finish_history
    切 live 流（live 冲击序列与前史长度无关）。`HistoryMeta{generated_through}`
    供任务 15 标 SeededPrehistory。
  - 保险无计息承载面：不注册利息 due（InterestAccrualWithoutDebtModel 类型化
    拒绝兜底）；赔案日程 = 保障日 elapsed%N 推导，不排队。
- **跨行业适用面矩阵（events.rs 头注钉死）**：需求字段=工商销量/保险新单/
  地产预售节奏，**银行存贷流不读**（K4 红线，金样字节级锁定）；成本字段=
  工商采购单价/地产开发投入；信用恶化=工商 ECL 率上浮+银行 assess_credit
  压力重估（保险/地产仅记录）；中断=工商停产+地产停工；减值迹象=工商固定
  资产一次性计提（激活当日）。
- **金样锚**：市场需求 +3000bp×10 日 → 工商收入恰 +300,000、保险现金恰
  +60,000、银行账套逐字节不变；行业成本 +1000bp 打化工 → 进项税差恰
  1,040（移动加权平均耦合的 1403 不做精确断言）；信用恶化 → 1231 恰
  5,650→33,900、银行 1303 -640→-16,005（含应计利息进 EAD 的 rhe）；
  停工 3 日 → FG 恰 16 vs 40、地产 1541 差恰 150,000；资金断裂 5 日现金
  轨迹 [100,0,0,126,52] 手算钉死。
- **教训（clippy 救命）**：手写 SplitMix64 常量抄错（0xBF58476D1CE4E5B9 丢 E、
  0x94D049BB133111EB 抄成 9D04…），自洽测试全绿发现不了（同错常数=同流）；
  clippy unusual_digit_groupings 的「位数不齐」提示直接暴露。**手抄魔法数
  常量后必须 diff 原件**。
- 测试目录形态：tests/company_operations/{main,fixtures,shock_gold,risk_gold,
  determinism,history_gold,seam,failures/{mod,scheduler,flows}}；QA 命令
  `cargo test -p engine --test company_operations` 21 测全绿；全量 773/0/4。

## 2026-09-11 W2-Task 13：行业财务报表、附注与版本化结账（accounting/reports 生成器 + closing）

- **模块布局（任务 15/18/29 接报表/披露时直接查这里）**：
  - `accounting/reports/`：`mod.rs`（185 纯行：ReportKind/Comparative/UnavailableReason/
    ReportVersion/ReportSet/ReportRequest/ReportSource/IndustryPresentation + generate 分发 +
    validate_trial_balance）、`error.rs`（ReportError 15 变体，Accounting/Consolidation 装箱
    ——task-12 DeclaredSide 先例）、`window.rs`（StatementWindows 窗口底座 + 单体累加器 +
    net_income_of）、`consolidated_window.rs`（Σ成员按代码→consolidate()→工作底稿折入 +
    少数拆分 PriorSplit）、`balance_sheet.rs`/`income.rs`/`cash_flow.rs`/`equity.rs`（五产物
    生成器）、`notes.rs`（归类表 = base ∪ 行业表 + 附注明细 + BsLine 标签表）、
    `validate.rs`（ReportSet::validate 勾稽 + verify_comparative_honesty + prior_year_facts）、
    `industrial.rs`（工业列报 + v2 扩充归类，四行业集合完成）。
  - `accounting/closing.rs`（248）：ClosingEngine 版本登记簿 keyed
    (ScopeId, period, kind)；close_month（试算→报表→勾稽→诚实性→封账→版本）、
    close_year（12 月封月 + Annual 版本）、snapshot_interim（Quarter/HalfYear 不封账）、
    correct（目标须已封账+有版本；调整分录须 > 目标且过账于开放期间；重述映射
    BusinessEventId→target period 生成新版本 supersedes 前版）、record（外部生成版本登记，
    合并 Scope 用）、version/versions（不可变查询）。
- **ReportSet 契约（任务 15 公开排期 / 18 个体认知 / 29 宿主 DTO 的消费面）**：
  - 五产物 = BalanceSheet（期末行 + prior_year_end Comparative；equity 拆 PaidInCapital/
    RetainedEarnings/MinorityEquity）+ IncomeStatement（quarter/cumulative 双栏五分类
    [经营/投资/筹资/终止经营 + 所得税单列，CAS 30 (2026) §32–38] + prior_year Comparative +
    少数/归母拆分 Option）+ CashFlowStatement（直接三类 + indirect: Vec<IndirectLine>）+
    EquityStatement（期初→净利→OCI 恒 0→投入→分配恒 0→期末；guardrail #4）+ Notes
    （items + 合并拆分披露 consolidation_split_items）。全部 serde 派生（K7 就绪），
  - 纯函数重建：generate_report_set(ReportRequest{period, kind, source, version, adjustments})；
    同 journal ⇒ 结构 + serde 字节双相等（测试锁定）。快照可由推进后的账套按窗口重建
    （窗口 ≤ P 天然排除后续分录）。
  - ScopeId：Standalone(MemberId)/Consolidated(root)；合并来源 = ConsolidationRequest
    （内部调 consolidate() 消费任务 12 输出；行业由成员 chart 版本推导 v1|2→工业 v3→银行
    v4→保险 v5→地产）。
- **双口径窗口语义（更正红线，任务 15 不要漂移）**：
  - **有效期间口径**（重述映射的目标期间）：BS/利润表/权益表/附注窗口；
    **实际期间口径**：现金流量表——现金属于实际收付期间，重述绝不双计；间接法以显式
    「重述调整对应现金流量」行配平（restated_cash_correction，无 plug）。
  - 间接法恒等式（cash_flow.rs 头注钉死）：`经营现金 = 净利 + Σ(非现金非损益科目 × −运动)
    − 投资现金 − 筹资现金`——由复式逐笔平衡 + 分录 CF 分类代数推出，四行业 + 合并金样
    逐分验证（银行放贷/保险赔款/地产交付等 NonCash 场景全部精确闭合）。
  - 更正后重述年报：BS 现金 = 原版 + 调整（有效口径）；CF = 原版不动（实际口径）；
    ledger.cash_total 与「实际运动合计」恒等（单次入账）。
- **归类纪律**：每个 chart 科目必须归入恰好一条主表行（UnclassifiedAccount 拒绝——
  chart↔行业错配即触发）；同码不同行 = DuplicateClassification；附注明细按行勾稽
  （BS 期末口径/利润表累计口径；Retained/Minority/PaidInCapital(合并)/计算行豁免——
  由恒等式覆盖）。合并归类 = 成员行业表按码合并：工业+银行可并（6701 同为减值损失行）、
  工业+保险不可并（1122 应收账款 vs 应收保费冲突）→ 类型化拒绝。
- **金样锚（分）**：工业 6 月月报：现金 97,300/资产 107,680/累计净利 2,595/当季 −785/
  6 月间接法 [−85, +25, +60]→0；年报：净利 5,595/经营 CF 5,300/筹资 4,975/间接法
  5,595+4,680−4,975=5,300。银行：贷款净额 6,060/累计净利 85/间接法当月 40。保险：
  合同负债 1,000（LRC 300+LIC 700）/累计净利 200/6 月 −700+700=0。地产：交付月净利
  2,000/间接法 2,000−(1,000+5,000−5,000+4,000... 见测试注释)=0。合并（80% 工业子）：
  净利 1,300（少数 60/归母 1,240）/权益 121,300（少数 4,060/归母 117,240/留存含子公司
  母公司份额 16,000）/经营 CF +400。生命周期：v1 年报 5,295→更正 v2 5,595、现金
  99,975→100,275、CF 5,000 不动、2031-01 月报 CF 恰 +300 一次、原版本 serde 字节不变。
- **坑**：开局凭证分类是 Financing 不是 NonCash（含现金行；任务 7 先例——NonCash 与
  现金行互斥会在 post_batch 炸）；任务 12 内部往来需要**镜像申报**（资产侧+负债侧各一条，
  单侧申报 = CounterpartyMismatch）；ytd/quarter 桶的窗口独立于报告窗口（月报的累计栏
  横跨全年——首版把桶嵌在报告窗口内，累计栏全变月度值，金样抓出）。

## 2026-09-11 W3-Task 15：定期报告、临时公告与不可变公开信息库（information/ + session/disclosures）

- **模块布局（任务 16/18/26/27/29 接披露时直接查这里）：
  - `src/information/`（6 文件，超计划字面 4 文件——250 行天花板，issues 已登记）：
    mod（87 纯行：模块根 + `InformationError` 22 变体 + 再导出）、schedule（113：
    `ScheduledReportKind`（landing_period/report_kind/publication_year）+
    `stable_company_offset`（seed+FNV1a(tag+id)→SplitMix64 终结器，无状态纯函数，
    每次重 derive 从不推进）+ `scheduled_instant`（基准日+偏移 18:00 + K4 契约守卫））、
    publication（238：`PublicationId`/`PublishedReport`/`Announcement`/请求类型 +
    校验族 ensure_*（origin⟺supersedes/scope 镜像/窗口/相位/时序/排期逐分吻合））、
    public_view（177：`PublicLibrary` 核心——publish_closed/publish_announcement/
    from_parts/save/自定义 serde）、queries（93：report/announcement/latest_report/
    for_company + EarlyRead 守卫）、prehistory（159：`assemble_seeded_prehistory`
    + `ensure_original_registered` + `industry_presentation`）。
  - `src/session/disclosures.rs`（143）：`disclosure_phase_observer`（生产 fn 指针
    相位钩子，无状态）+ `DisclosureDispatch`（published_through/announced_through
    双游标 serde）+ `run_day_end`（公告先于定期报告；公司 id × 种类确定序）。
- **PublishedReport/PublicationId 契约（任务 16/18/29 的消费面，不要漂移）：
  - **单一真源**：scope/period/kind/window/version 全部由内嵌 `ReportSet` 承载，
    PublishedReport 顶层不重复这些字段（恢复边界零一致性检查）。
  - **PublicationId 库内单调**，报告与公告共享一个计数器；`from_parts` 恢复边界
    全量校验：逐条 ensure_report_shape（含 `set.validate()` 勾稽）+ id<next_seq +
    计数器与最大 id **严格衔接**（幻影分配 = InconsistentLibrary）+ 重复 id 拒绝。
    serde 自定义（save DTO → from_parts，PlanBook 先例）。
  - **publish 校验顺序**（错误优先级由测试钉死）：origin⟺supersedes → scope 镜像 →
    排期窗口（landing_period 匹配 + 偏移域）→ 相位 18:00 → publish≥approve →
    approve 严格晚于期末 → 排期时点逐分相等 → supersedes 目标四元组匹配 →
    登记簿取版本（未定稿拒绝）→ set.validate。**窗口检查必须先于时间检查**
    （否则 6 月期请求先撞 ApprovalPrecedesPeriodEnd 而非 IllegalWindow）。
  - **EarlyRead 是 by-id 查询守卫**（as_of < published_at → 类型化拒绝）；公司面
    查询按 as_of 过滤不报错；latest_report 取 max(id)（更正 id 更大 ⇒ 天然最新）。
  - origin 变体：`SeededPrehistory{fy,kind,offset}` / `ScheduledDisclosure{同字段}` /
    `Correction`（⟺ supersedes.is_some()）；前两个共用排期字段集，恢复边界重验
    排期吻合。批准时点约定 = 公布日 08:00（`APPROVAL_HOUR` 常量，单一约定源）。
- **前史装配契约**：`assemble_seeded_prehistory(config, game_start)` 内部跑
  generate_history（as_of = 前史首日前一天，无借款账套调用方契约）→ 收集
  instant < start 00:00 的全部排期 → (instant, company, kind) 全局排序逐条
  「登记 + 公布」⇒ 同 seed PublicationId 序列 + 库 serde 字节逐位一致（金样锁定）。
  1998 年报的上年比较项 = **Available**（开局凭证 1997-12-31 落在上年），
  1998 中期 = **Unavailable**（上年同季窗口早于开局凭证）——任务 13 的窗口级
  判定语义，勿按「1997 无账」直觉断言。
- **版本登记策略（结构性约束）**：行业账套（IndustrialBooks 等）只暴露只读
  `books()`，`close_month/close_year`（需 &mut Books）从 information/session
  **结构性不可达** ⇒ `ensure_original_registered`：Q/H1 走 snapshot_interim
  （不封账定稿快照），Annual/Monthly 走 generate_report_set + record()（外部版本
  登记，record 自带 set.validate；补 verify_comparative_honesty + prior_year_facts
  ——两者都是 pub/pub(crate)，in-crate 可用）。封账接线归任务 26（issues 登记）。
- **坑（本轮真实踩过）**：
  - `stable_company_offset` 首版写成 `% 7 + 1`（偏移域 1..=7，0 永不可达）——
    边界测试抓 0..=7 域时靠金样断言；手写取模后必须过一遍全域样例。
  - **crate 内兄弟模块不可达 private 路径**：`crate::company::spec::CompanyId`
    在 company 内部合法但 information/session 里 E0603——必须走再导出
    （`crate::company::CompanyId`）。同理 accounting::period/journal。
  - **跨文件 impl 的字段可见性**：PublicLibrary 定义在 public_view、查询面在
    queries（兄弟模块）⇒ 字段 `pub(in crate::information)`（execution 深度模块
    `pub(in crate::session)` 先例）。
  - 工业开局 fixtures 的 opening_lines 与 opening_inventory/opening_assets 必须
    逐科目对账（OpeningSeedMismatch：1601 有余额无资产种子即炸）。
  - 测试断言 CivilInstant 无 `.year()`——先 `.date()` 再取。
  - cargo test 只接受单个 TESTNAME 过滤参数；多过滤词分多次跑。
  - 首次全量跑出现过一次 1-test 失败（位置=insurance_accounting，名字未捕获），
    之后两次全量 + 单套件隔离均绿——按环境抖动如实登记（issues）。
- 全量 `cargo test -p engine`：814 过/0 败/4 忽略（基线 791 + publications 22）；
  新代码 clippy 0 警告、fmt 干净；`cargo check -p engine` 0 警告。


## 2026-09-11 W3-Task 16：公共曝光与个人获知分离（information/{acquisition,npc_view}）

- **模块布局（任务 18/25/26 接个人认知/注意力/会话时直接查这里）**：
  - `src/information/acquisition.rs`（208 纯行）：`NpcInformationState`（owner
    `AccountId` + `BTreeMap<CompanyId, Vec<AcquisitionRecord>>`；每公司 Vec 按 id
    严格递增 = 恢复边界校验的不变量，也是二分查找前提）+
    `record_acquisition(npc, &library, id, observed_at)`（守卫顺序固定：属主 →
    库存在+observed_at≥published_at → 幂等）+ `AcquisitionOutcome`
    （Recorded/AlreadyAcquired——重复获知是**合法幂等输入**，保留首次时点、
    状态字节不变）+ `AcquisitionError`（Library(#[from] InformationError)/
    OwnerMismatch/NotAcquired/InconsistentState）+ `discovery_candidates(
    library, company, as_of)`（公共曝光→候选 id 面，任务 25 消费；候选≠阅读）。
  - `src/information/npc_view.rs`（80 纯行）：`NpcObservationContext<'a, Market>`
    ——四输入构造（npc、&NpcInformationState、&PublicLibrary、&Market 泛型），
    全引用零克隆；`acquired_reports()/acquired_announcements()`（公司序+id 序）
    + `report(id)/announcement(id)`（as_of=**首次获知时点** ⇒ 版本钉死；未获知
    ⇒ NotAcquired）+ `market()`。
- **关键语义决定（任务 18/25/26 不要漂移）**：
  - **历史版本钉死**靠「更正=新 id + 读时 as_of=首次获知时点」实现：获知 v1 后
    更正 v2 公开，上下文仍只暴露 v1（字节不变）；v2 需要新的获知事件。不需要
    存任何内容副本——PublicationId 就是版本。
  - **公共曝光 ≠ 个人阅读**：`discovery_candidates` 只投影公开索引（as_of 过滤、
    id 序）；注意力接线（任务 25）用候选定位，真正的获知必须走
    `record_acquisition`。dev/宿主查看 = 公开库查询面（&self），对个人状态
    零写入（字节对比测试锁定）。
  - **无前视金样的测法**：judgment_projection = 已知 id 集 + 获知时点 + 每条
    内容 serde 字节（报告前/公告后，公司序+id 序）——「改变未披露事实（开放期
    间过账）或已披露未读（更正公开）⇒ 未读 NPC 投影字节不变」+ 甲真获知后
    投影必变的区分力对照。
  - **守卫透传不重复实现**：unknown id / observed_at<published_at 直接复用任务
    15 查询守卫（report() 先查、UnknownPublication 再查 announcement()；EarlyRead
    原样透传，错误上下文完整）。
  - **存档**：`NpcInformationStateSave`（owner+companies）+ from_parts 校验（公司
    内严格递增 + 跨公司 id 全局唯一）+ 自定义 serde（PublicLibrary 先例）。
- **Market 泛型的边界**：可见行情输入是泛型参数——session（任务 26）装配真实
  快照实例化；信息域类型面上不可达 CompanyState/Books（构造面只有四输入，
  failures.rs 有最小构造测试钉死）。
- **PowerShell 再踩第三次（真损毁）**：`Get-Content -Raw` + `-replace` +
  `Set-Content -Encoding UTF8` 对 write 工具写的 UTF-8 中文源文件 = GBK 双重
  编码 + BOM + 有损 `?` 替换（acquisition_gold.rs 全部中文注释毁掉，靠上下文
  整文件重写恢复）。**铁律重申：源码/notepad 改写一律 edit/write 工具；字节级
  追加/拼接用 node fs 或 cmd /c copy /b；PS 管道碰都不要碰。** 另：PS `>>`
  追加会把输出重编码 UTF-16（证据文件 append 中招，node 重建）。
- 测试目录形态：tests/information_acquisition/{main,fixture,acquisition_gold,
  view_gold,failures}.rs（13 测试：金样 6 + 拒绝 7）。全量 `cargo test -p engine`
  828 过/0 败/4 忽略（基线 815 + 13）；新代码 clippy 0 警告、fmt 干净、
  `cargo check -p engine` 0 警告。


## 2026-09-11 W4-Task 25：个人关注发现、保留与淡出（attention 发现权重 + watchlist）

- **模块布局（任务 26/27 接线时直接查这里）：**
  - `src/experience/watchlist.rs`（105 纯行）：`PersonalWatchlist{stocks: BTreeMap<StockCode, WatchedStock>, latest_attention_minute}` + `WatchlistError`（TimeWentBackwards/ObservationInFuture/InconsistentState）。`record_attention(code, minute, now)` 双守卫（未来观察→now、回拨→列表时钟；条目≤时钟不变量由登记路径+恢复边界共同维护，单检查即覆盖单条目回拨）；`prune(&protected)` 与 price_memory 同款降序 (minute, code) 保留前 8，**protected 不占 8 上限**（持仓∪活跃计划永不驱逐）；`from_parts` 拒绝条目分钟>列表时钟（修剪历史允许时钟>现存条目）。自定义 serde 走私有 DTO（PlanBook 先例）。
  - `session/attention.rs` additive 追加：`NpcAttentionState::discovery_weights(market, exposed)`（关联函数）+ `sample_discovery_stock(&mut self, market, held, watchlist, exposed)`。**可达性关键：新公共 API 必须挂在已 re-export 的类型上**（attention 是 session 私有 mod，session.rs/lib.rs 都不能改——NpcAttentionState 已经 `pub use`，impl 块放 attention.rs 即成公共面）。
- **发现权重契约（任务 26 不要漂移）：** 基础 1.0 + 异常30分钟涨跌(|Δ|≥2%, 最近≤30 完整分钟窗, 涨跌对称, <2 样本或首价≤0 无信号)+1.0 + 相对量能(≥2.0, 非有限=无信号)+1.0 + 公告曝光+2.0，最大 5.0。抽样结构继承 behavior select_observed_stock：60% 持仓优先（**均匀，不吃权重**）→ 70% 关注列表（加权）→ 全市场（加权）；各分支 RNG 消费次数与原版同构（门 f64 + 选择 1 次），个体 RNG 流 = NpcAttentionState.rng_state（evaluate_attention_candidate 同款 save/restore 模式）。
- **曝光输入是调用方组合面**：`exposed: &BTreeSet<StockCode>` 由接线从 task-16 `discovery_candidates` 派生（公司↔股票映射与曝光新鲜度窗口归任务 26 政策）。私有经营事实结构性不可达——输入只有 MarketView+曝光集合；金样：Books 过账未披露分录前后 weights/候选序列/会话事件与存档字节全等。新曝光≠已读：抽样 1000 次后 NpcInformationState 字节不变（acquired_count=0）。
- **分布金样的测法**：固定 seed 全序列确定性 + 5σ 容差断言（σ=√(np(1-p))）。手算期望时两次踩坑：0.6·0.5+0.4·(2/3)=0.5667（首算 0.7667 错）、watchlist 单元素池 P=0.7+0.3·(2/3)=0.9（首算 0.8667 错——70% 门进单元素池必中）。写期望前先列三分支加权和再落笔。夹具注意：watch_with 按时间顺序登记（列表时钟单调），倒序登记直接 TimeWentBackwards 拒绝。
- **rustfmt 1.96 新坑**：--edition 2021 仍默认 version-sort import（SCREAMING_CASE 排 CamelCase 后），需要 `--style-edition 2021`；且 rustfmt 文件参数会**递归格式化 mod 子模块**（experience.rs 把 feedback.rs 也改了）——格式化后必须 git diff 检查并 checkout 非自己文件。
- 全量 cargo test -p engine exit 0：872 过/0 败/4 忽略（基线 828 + 本任务 20 + 并行 task-18 的 fundamental_beliefs 24；其间 lib 被对方中间态打断约 90s 后自愈）。新代码 clippy 0 警告。


## 2026-09-11 W3-Task 18：个人基本面预测、估值与信息驱动更新（strategy/fundamental + beliefs）

- **模块布局（任务 22/23/26 接信念/聚合时直接查这里）**：
  - src/strategy/beliefs.rs（122 纯行）：BeliefInputs{ctx, company, kind, total_issued_shares,
    as_of_trading_day}（行情不进估值）、BeliefError 7 变体（Acquisition/FactsUnavailable 透传
    + MaterialNotForCompany/MaterialNotAnnual/HorizonNotElapsed/NoBeliefEntry/NoOwnAnnualMaterial）、
    BeliefEntry{method: Option, forecast, confidence_bp, valuation, used_report_ids,
    anchor/horizon, last_cause, applied_experience_orders}、BeliefBook{npc, profile, analysis,
    assumptions, entries}+apply_cause 分发。字段 pub(super)——更新语义在 fundamental/update.rs
    的 impl BeliefBook 块（任务 3 的跨兄弟模块先例）。
  - src/strategy/fundamental/（5 文件）：mod（197：PersonalAssumptions 六字段 pub +
    draw_personal_assumptions canonical 6 抽序 = 字段声明序、ValuationUnavailable 16 变体
    serde（Overflow.step 是 Cow<'static,str>——&'static str 不能 derive Deserialize）、
    ValuationOutcome/PerShareRange、CapabilityCenter + λ/horizon 映射、per_share_price/
    to_per_share_range/estimate_by_method、本地 div_round_half_even 第 5 副本）、facts（88：
    extract_annual_facts 守卫序 = 公司→种类→未来；归母优先读取 + 借款行附注运动）、forecast
    （86：GrowthObservation/ForecastBasis/ForecastState + observe/initial/revise）、valuation
    （203：三法 + FCFE 推导 + DCF 逐步 rhe）、update（247：BeliefCause/CauseRecord + 信心/
    期限 + 四个 apply_* 方法）。
- **K5a 关键决定（任务 22/23 不要漂移）**：
  - **FCFE ≡ 年度净现金变动（代数恒等，valuation.rs 模块文档钉死推导）**：capex = −投资CF；
    新借−还本 = 借款行（按 BsLine 归类不按裸码）附注运动取负；现金利息 = (新借−还本) − 筹资CF
    （经营CF 未扣利息 ⇒ 需再扣）。利息拆出负值 ⇒ FinancingSplitUndeterminable（窗口含开局
    凭证等不可解释筹资流入——首年之外的正常年份恒可拆）。
  - **个人假设一次性抽样**：BeliefBook::new 注入 &mut dyn Rng 恰抽 6 次 f64（canonical 序），
    之后 apply_cause 全系不收 RNG；测试 SeqRng 第 7 抽即 panic 锁死抽样数。任务 26 接线必须用
    独立种子派生的专用流（extraction_replay 锚点教训，同任务 17 的 derive_analysis_profile）。
  - **无 per-tick 更新**：唯一变更入口 apply_cause(stock, cause, inputs)；无触发 ⇒ 调用方不调
    （字节稳定由 no_trigger 测试用 serde JSON 字节对比锁定）。
  - **中心/λ/期限表**：λ——Retail(LongTerm)+机构 DeepValue/Defensive=4000、机构 Growth=6000、
    其余（含 Balanced）=2500；期限——长期/价值 60、Balanced/机构 Growth 20、其余 5。PE 档——
    DeepValue 8–16、Defensive 6–12、其余 10–24（抽样在 pe_range_for，任务 17 档位语义）。
  - **首年年报的先验是 Degenerate 而非 WithoutHistory**：开局凭证落在上年 12 月 ⇒ 上年比较项
    Available 但收入行为 0 ⇒ 零收入分母 ⇒ GrowthObservation::Degenerate（K5 行 133 显式退化）；
    WithoutHistory 仅当比较项 Unavailable。Degenerate ⇒ CashFlow 法 GrowthPriorUnavailable，
    EM/ROE 不消费增长照常可用（方法间解耦是有意语义）。
  - **修订语义**：λ 混合 new=rhe((10000−λ)old+λ·obs,10000)；observed 缺历史 ⇒ 不修订（保留旧
    预测，仍按新事实重估）；observed 退化 ⇒ 修订后退化；旧退化+新观察可得 ⇒ 以新观察作初始先验。
    Correction/CreditDefault ⇒ 直接重估（丢弃旧预测、信心重置初始规则）；HorizonExpired ⇒ 同
    事实重估、预测/信心不动、锚推进；经历 ⇒ 只动信心（−1000/+500、0..=10000 饱和、同订单去重
    via applied_experience_orders）。
  - **估值只除已发行总股本**：BeliefInputs 根本没有流通股字段；「改流通不变」金样用两个不同
    market 夹具跑出字节相同的信念簿证明。每股 = rhe(归母整体估计, 总股本)，越 i64 ⇒
    PerShareOutOfRange。
- **金样独立推导法**：期望值全部由独立 Python 脚本（临时目录，rhe 半偶重实现 + spec 公式
  直译）预先算出再硬编码进测试注释——与 Rust 实现零共享；DCF 金样：FCFE 17,820,000 分、
  g=0/r=1000/gt=0 ⇒ 中央 191,648,179 分（五年 PV 各 16,200,000 + 终值折现链）。乘法逐位复核
  教训（任务 9）由脚本代劳，但脚本本身手写公式仍需对照 spec 行号。
- **same-news 相反修订夹具**：需要 4 个年度账套（2027–2030）——甲读 FY2029（爆发 clamp
  2000+dev1000=+30%）、乙读 FY2028（−20%+dev−1000=−30%）、同读 FY2030（+10%）⇒ 甲
  3000→2500 下调、乙 −3000→−2000 上调。同公司同期先验只能靠 dev ±1000 分离（≤2000bp 差），
  要 ±60pp 分离必须让两人读不同历史。
- **共享工作树**：任务 25 agent 期间提交了 attention/discovery 并留下 strategy/{analysis_
  profile,factory_profiles,sampling} 与 policy-sources.json 的 WIP；我只 stage 自己的 16 个
  文件（staging 显式枚举），rustfmt 只跑自己的文件。全量 cargo test -p engine 在其中间态
  exit 0（872 过/0 败，36 套件；基线 2d15e7c 为 828 过/4 忽略）。

## 2026-09-11 W4-Task 23：观点之外的执行紧迫度与受保护报价

- **模块布局**：`plans/urgency.rs` 只做紧迫度、暂停触发可用性与恢复判断；版本化
  `UrgencyPolicy` 独立放 `plans/urgency/policy.rs`，serde 恢复时立即校验版本和阈值。
  `plans/quote_policy.rs` 只输出 Wait/Keep/Cancel/Replace/Submit 意图，路由镜像守卫拆到
  `plans/quote_policy/validation.rs`。所有新源码低于 200 纯行。
- **K5a 边界**：买计划只有 30 分钟 `<= -300bp` 且 1 分钟 `<= -75bp` 同时成立才请求
  暂停撤单；任一窗口缺失保留 `Unavailable { reason }`，不能等价成零收益或撤单。风险减仓
  回撤 `>=2000bp`、剩余期限 `<=1` 日为 Urgent；信心 `<4000bp` 或 LongTerm/DeepValue 为
  Patient。恢复面单独要求下一本人观察、触发器 Clear、`S>=2000`。
- **报价与路由边界**：Patient 取同侧最优价，Normal 仅在双边簿取对手价，Urgent 使用调用方
  传入的受保护限价；价格、数量、涨跌停带、tick、价格笼子均在输出前按现有 session 路由
  语义只读预校验。模块不持有 OrderBook、不生成成交、不调用撤单 API；Replace 明确报告
  QueuePriorityLost，不可撤阶段 Keep + PendingReconsideration 并抑制新报。
- **验证**：urgency 27/27；完整 `cargo test -p engine` 全绿（含并发任务 22 的
  plan_allocation 22/22）；`cargo check -p engine` 绿。clippy 对本任务文件零新增警告。
- **独立复核修复**：首轮 REJECT 发现卖单零股余数规则虽镜像路由，但遗漏了路由更早的
  `qty <= available_sell` 守卫，150 股可卖时会错误放行 250 股（同为余 50）。先加失败测试
  实证，再补库存上限；复核确认 150/150 可报、151/150 与 250/150 均拒绝，结论 APPROVE。

## 2026-09-11 W4-Task 22：账户内候选评分与软预算

- **模块布局**：`plans/candidates.rs` 聚合五路 K5a 信号，子模块分别持类型、整数信号数学与
  目标换算；`plans/allocation.rs` 只做账户内确定性排序与预算编排，子模块持版本化政策/请求类型
  和 task-20 经历读缝。公共入口仍为 `engine::plans::*`。
- **预算真源**：调用方传入权威 cash、live-order frozen cash、equity 与逐请求最低费用；分配器只对
  尚未报单部分分软预算，先扣冻结且不接收预计卖款字段，因此不会重复冻结或预支换股收入。
- **确定性顺序**：风险减仓（含长期被套卖出）→已有计划→新机会；同类按经历折减后的信心降序，
  再按稳定 PlanId 破同分。正反输入顺序得到完全相同 AllocationResult 的测试已锁定。
- **整数纪律**：SignalScore 限于 [-10000,10000]，不可用项剔除并按可用原子/维度权重重归一；
  所有评分、目标权重、现金保留和股数换算使用整数 half-even。目标股数超过 u32 不再静默封顶，
  而是 `CandidateError::ArithmeticOverflow`。
- **独立复核修复**：分配批次先拒绝重复 PlanId，避免完整同排序键退化为输入顺序；目标股数换算
  固定校验 `A_SHARE_BOARD_LOT=100`，不能由调用方传 1/200 绕开大 A 语义；新增正负半偶与现金分
  半偶边界。复核结论 APPROVE，focused 28/28。
- **源码体积**：`allocation.rs` 205 纯行、`candidates/signals.rs` 242 纯行，均处于 200–250 warning
  band；下一次扩展前应优先按预算阶段/信号族继续拆分。其余任务 22 新源码低于 200 纯行。

## 2026-09-11 W4-Task 24：以真实订单生命周期执行持续计划

- **模块布局**：`session/plan_execution.rs` 是显式 observation 编排入口；`types.rs` 持公开请求/
  结果/类型化错误与内部生命周期事实；`actions.rs` 只做认领、submit、cancel、replace；
  `routing.rs` 复用既有连续/集合竞价路由并维护 `ParentOrderPlan`；`synchronization.rs` 以克隆
  `PlanBook` 事务化应用 accepted/fill/day-end 回写。原 359 行入口已按责任拆分，新文件均低于
  250 行。
- **唯一真相**：执行器不建第二套订单、冻结或成交系统。`ParentOrderPlan.linked_plan_id` 只标记
  显式计划所有权；accepted/fill 由既有 router/settlement 的记录接缝产生，只有真实 fill 推进
  `filled_qty`。同账户同股票已有不兼容母单或在途子单时拒绝创建第二个。
- **报价合同**：旧工作单只在账户、股票、方向、价格、合法子单数量全部精确一致时认领并保留
  OrderId；变价先真实撤单再真实提交，得到新 OrderId。09:20 后开盘集合与收盘集合不可撤时仅
  返回 PendingReconsideration，不发矛盾单；撤单失败保留旧工作单与原冻结。
- **日终合同**：日终仍由既有订单释放和冻结释放路径清空执行子状态，计划本体保留；下一交易日
  只有下一次本人观察才重报。若集合成交已经达到 target，同一边界不再追加 day-end 事件，避免
  Completed→day-end 非法转移（新增回归测试）。
- **回放金锚未变**：默认会话没有 linked plan，`pending_plan_events` 不入档，`linked_plan_id=None`
  由 serde 跳过；`extraction_replay` 3/3 通过，事件 FNV=8_666_897_876_443_600_996、mid-save
  FNV=1_702_442_567_969_422_992、end-save FNV=190_030_750_827_517_148。focused 10/10；全量
  engine 937 通过、4 ignored；check 与 scoped clippy 均 exit 0。

### W4-Task 24 finish pass 补记

- 复核收尾补两枚验收锁测试后：focused 12/12（gold 3 + failures 9）、全量 engine
  939 通过 / 0 败 / 4 忽略、`cargo check -p engine` 0 警告、scoped clippy（`--lib` 与
  `--test plan_execution`）exit 0。锁测试：在途子单唯一性（第二个 Submit →
  `IncompatibleExecutionState`）与集合竞价转连续同 id 交接（`stage_auction_remainders`
  保留 `OrderId(arrival_seq)`，`Keep` 认领成功）。
- `--all-targets` clippy 在本任务范围外仍被任务 17/20 的测试 lint 阻断（详见 issues.md
  补记）；本任务文件零新增警告。


## 2026-09-11 W5-Task 26：接通完整决策链并彻底删除共同 V

- **V 删除清单（全部落地）**：market.rs（VParams/fundamental_value/v_initial/evolve_v/
  set_fundamental_value/round_half_to_even_f_to_i64；InvalidVParams→InvalidParams）、
  compute.rs（evolve_v_all 与 with_v/no_v 双视图——decide_all 只收单一公共 MarketView）、
  strategy/{mod,value,institution,factory,params}.rs（StockView.fundamental_value、
  needs_fundamental_value、TargetPolicy::TrackV、ValueStrategy；新增
  BeliefInstitutionStrategy + Strategy::belief_chain_params 能力探针 +
  BeliefChainParams）、session.rs（SessionSetup.v_params/fundamental_value_means/
  StockSpec.v_initial、step 的 V 演化段与 Event::VError、restore 的 V 回填、全部 fixture）、
  views.rs/snapshot.rs（MarketSnap.fundamental_value）/persistence.rs、lib.rs 再导出、
  engine-gpu（evolve_v_all + shaders/evolve_v.wgsl 文件删除）。diagnostics 的 VError 臂删除。
- **决策链接线（session/decision_chain.rs）**：step 串行段对本次 accepted 注意力
  中暴露链参数的机构账户执行：个体发现（消费注意力个体流）→ 候选集（持仓∪信念条目∪发现）→
  discovery_candidates + record_acquisition → 新年报 BeliefCause::NewMaterial + 到期
  HorizonExpired → K5a 五路信号（fundamental_signal 用每股区间；trend/pv 用 price path；
  technical 用 build_technical_observation——观察日 = candles.len() 使预置历史天然早于
  第 0 交易日；机构成本经历诚实标不可用）→ blend_candidate → 计划开/修订（反向跨
  ±2000bp 迟滞）→ allocate_soft_budgets → assess_urgency → decide_quote →
  execute_plan_observation（真实路由）。曝光集合新鲜度窗口 = 2 自然日。
- **新局装配（session/company_assembly.rs）**：默认表命中条件 = 代码 + 股本**逐字相等**
  （股本不匹配的 setup 用表内数字会造成每股账面值数百倍失真——1M 股测试局实测翻车后修的）；
  未命中走通用推导（现金 30%/固定资产 80%/短借 10%、各科目至少 1 元、授信
  max(1元, 20%)）。经营流 = 高周转游戏假设：年收入 ≈ 4×资本×代码哈希倍率（1..=6）、
  原料 2400..=2800 分——**同构公司会让全体机构单边看空，代码哈希差异化是多空分歧的
  来源**（3 天集合竞价 0 成交 → 加差异化后 1500/1500/4000）。税务 13/13/25/5y 显式
  版本化。公司域种子 = seed ^ FNV1a("company-operations")。
- **populate_npcs 的 RNG 纪律**：derive_analysis_profile 用 seed ^ FNV1a("analysis-profile"
  +id)、BeliefBook 六抽用 "belief-assumptions"——均独立 SplitMix64 流（决策链同款
  derived_stream helper）。
- **end_civil_day K4 全序**：时钟日结 → ops_wiring.run_day_end（经营终局+增量同步+镜像
  裁剪 prune_dispatched）→ close_accounting_periods（月末 close_month/年末 close_year，
  经 books_mut 接缝：IndustryBooks::books_mut→Industrial 分支，银行等分支
  unreachable 诚实标注）→ DisclosureDispatch::run_day_end。SessionError 新增
  CompanyOperations/Disclosure/Closing/StrategyAnalysis 透传。
- **restore 过渡边界（重要）**：GameSession::new 全量重建（前史确定性），然后
  **重放经营** while next_expected < current_date（严格小于！含等号会提前结算当天，
  DateOutOfSequence 炸）+ ops_wiring.adopt_all_pending（重放后调度器 due id 与原序
  一致，直接收编镜像集，**绝不 install 重注册**——会重复注册 due 使时钟字节漂移）。
  信念/计划/信息集/关注列表恢复后复位；已链接母单 linked_plan_id 剥离防死引用。
  两个逐步字节连续测试按过渡契约改写（profiles/注意力等价），issues 登记。
- **synchronize_plan_execution 容错化**：pending 队列现承载会话簿 + 外部簿（测试自带
  PlanBook）混合事件：只应用「本簿存在且未终止」的计划事件，未知/已终止条目**保留**
  给属主；成交后终止仍清理链接母单。外部簿的迟到事件不再误报 UnknownPlan。
- **执行循环的三个坑（连环修）**：(1) 快照 remaining 与 execute 内部同步后 remaining
  错位 → 循环顶部先 sync；(2) Urgent 保护限价必须同时夹到涨跌停带**和连续竞价价格笼子**
  （否则 QuoteError OutsidePriceCage panic）；(3) desired_qty 卖出要 min(max_order_qty)
  （千万股级 float 会让 5300 万股子单撞 100 万申报上限）、拨款不足（allocated 0）时
  本轮跳过（分配器承载无资金语义，不 panic）。
- **市场必须自证活着**：allocated_market/all_stocks 回归最初 0 成交——单机构
  NonPositiveNetIncome（真实游戏动态）+ 人口太薄；inst_count 1→5（五风格轮换）后
  恢复成交。单股无对手盘局 0 成交是**期望行为**（failures 套件锁定）。
- **extraction_replay 重钉**：events 7_100_597_875_750_696_841 / mid
  18_072_312_056_192_250_746 / end 5_864_974_982_281_894_531（旧值在测试注释+issues）。
- **rustfmt 递归坑再现**：显式文件参数会格式化 mod 子模块——experience/fundamental/
  sampling 等非本任务文件被重排，git checkout 还原后手动重加 beliefs.rs 两个访问器。
  格式化后必须 git status 全检。
- **QA 硬门禁全绿**：company_decision_session 11/11（含跨线程确定性：RAYON_NUM_THREADS
  1 与 8 都过钉锚）；cargo test --workspace exit 0；clippy lib+touched 目标 0 警告
  （analysis_profiles/experience_feedback 既有测试债不变）；rustfmt 对自有文件通过。
