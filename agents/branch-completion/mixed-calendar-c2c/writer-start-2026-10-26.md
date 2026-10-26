# C2-C writer 准备阶段接棒回执

2026-10-26。当前只接收 fixture、测试及非共享准备写权；中央生产接线尚未释放。

## 实际身份与冻结起点

- 实际 agent：`/root/sol61_mixed_calendar_design/mixed_calendar_c2c_writer`；派工指定 `gpt-6.1-sol/high`，未替换模型。
- 独占 worktree：`/data1/baiyifan/workplace/stock_market_game/.worktree/mixed-calendar-c2c`。
- 分支：`feat/engine-mixed-calendar-c2c`；实际创建起点 `d7a26186d40ec38ffce901f48a935917b5d29641`。
- 任务原件位于根工作区 `agents/branch-completion/mixed-c2c-production-task-2026-10-26.md`，SHA-256 `e70821fdbbccd13248eb7bccc66ad4086221542bc0c969351706845ac7b19116`；独审 SHA-256 `a2b154a62175e3570e60607cc23c159b3461de80f521126d5c8f415122446efe`，均已实算核对。
- I-D 实际已审 tip `3984e14167202dc645ec340fbd2a73e661c71c8a`；C2-A `5225843ced76bd70418661ee47d190e372e7b699`。当前没有自行叠入，等待唯一 main writer `/root/sol61_integrate_asm_final` 给出准确整合 ref。
- 根工作区旧 `af0fdeb6` 与其未提交资料均未触碰；本树不复制 target，不运行 Cargo/all。

## 准备阶段精确写入白名单

1. 新文件 `packages/engine/tests/mixed_calendar_c2c.rs`：独立 integration test 入口，Cargo 自动发现，不改 `session.rs` 或任何生产模块声明。
2. 新文件 `packages/engine/tests/common/mixed_calendar_c2c_fixture.rs`：仅供上述测试通过 `#[path]` 引用的 fixture/helper，不改现有 `common/mod.rs`。
3. 本目录新增证据：本回执、`fixture-contract-2026-10-26.md`、真实命令/输出日志、源码/产物摘要及之后明确命名的 red/green 证据子目录。

当前不修改 `runtime/{drive,group,world,desk,protocol,coordinator,bridge,mod}.rs`、`session.rs`、`session/failure.rs`、pipeline 或生成类型/Web 文件。生产白名单须在 INFO-P/D1 最终合同核对与中央写权释放后另记；独立 worktree 本身不解除此锁。

## 真实 fixture 方案

- 两股 `600101`/Shanghai、`000101`/Shenzhen，明确零 NPC、`ticks_per_day=2`、无竞价窗口、初始 `tick=0/day=0`；竞价专项另用 `12/3/3`。
- 先由现有 `GameSession::new` 构造合法初始局并 `save()`；只在这个尚未运行市场的 `SaveSlot.civil_clock.policy.official_coverage` 中加入 synthetic 所/2030 年覆盖，再走真实 `GameSession::restore`。
- 初始 `2030-01-02`；所覆盖关闭区间包含 `2030-01-01..=2030-01-02`，以保留初始化前史的 Jan1 休市事实；来源与 digest 明示 `synthetic-test`，不冒称官方公告。
- 正反两所差异及 setup 顺序置换都重建合法新局，StockId 通过实际 catalog 查询，不在存档上交换数组。恢复后只在内存推进市场/自然日，不运行后换 policy/source/owner，不新增生产政策开关。
- 初始 restore 拒绝时保留具体错误作为 fixture 失败；不以新 API 编译失败冒充行为红，也不通过放松恢复断言补假 K。
- 首批目标：初始 restore 正控；closed 首拍零 PriceTick/active/分钟，open 有真实行情；两拍日终 closed 仍 360 日 K、open 361；复市本股 ordinal/minute 与共享钟分离。尚未运行这些测试，不报告红或绿。

## 待最终冻结的类型与接缝

- INFO-P：`InformationPreparePayload`、`InformationObservation`、`PreparedInformationResult`、`AdminGroupCandidate` 的实际最终字段/visibility；`World::with_information_transaction` 签名；current_facts 独立更新、`observation=None`、empty locals/all-live 组行为与其批准记录。当前仅只读候选，不复制 `62ba8666` dirty 或假设其终签。
- diagnostics D1：实际最终 driver candidate prepare/commit/cleanup 接口与 observation 的 before/after 字段来源；`35158546bd3863c63148b1e7f3470de38c326f3f` 只属中间代码，不单独合 main，不用旧候选签名实现中央接线。
- I-D：main 精确整合后的 PlanId owner-local 游标与 ExternalPlan/receipt 签名；保真实受理 ordinal/FIFO/None 耗尽。
- C2-A：最终 main 中 `MarketCalendarSource/MarketDayContext/MarketTickContext` 的 blob 与 visibility；准备使用同一 Session Tick Arc，并在开放生产窗后增加已批窄 stock 验证。
- #122：目前 E1，无已存在中央 candidate API；保持 D0 已批回滚，不预置 Full swap。
- SOURCE/R4：仅消费已有 current facts/单可选 observation 合同；不定义 closed as_of、不新增 hook。

## 已读领域依据

本轮实读归档 SSE 2026 §2.4.1、SZSE 2026 §2.3.1：各自法定假日及本所公告休市；SSE §11.9 明确 2026-07-06 施行。归档原件与 URL/SHA 以根工作区 `mixed-calendar-sources/manifest.json` 为准；synthetic 2030 fixture 仅证明已批准双所模型，不声称真实 2030 公告差异。本片不改竞价制度、税费或结算模型。

完成声明：目前仅完成独占树建立与准备范围登记；生产、行为红、build/run、独审均未完成。
