# Luna66：hosts fixtures2 / performance / production caller EOF 复核

目标产品提交 `08e4fc7`，审计 worktree merge HEAD `a7c7ce3`（merge 同）。已读根 `AGENTS.md` 与 `docs/principles.md`。对三个指定审查文件全文连续读至 EOF：`hosts/review-fixtures2.md` 71 行、`hosts/review-performance.md` 76 行、`hosts/review-production-entry-caller.md` 22 行。只读核对目标提交源码、caller、性能脚本入口与既有结论；没有改产品/Git，没有运行测试、编译、真实性能或长测。本次不把 OOP 重构范围当作正式现行约束的豁免。

## EOF 章节矩阵

| 文件 / 原行 | 原文主张 | 当前源码复核 | 结论 |
|---|---|---|---|
| `review-fixtures2.md` 1–14 | 基线、静态门禁、未声称运行测试；六动作限定测试 fixture，A 股规则未改 | 六动作及 12 个测试源文件在 `fixtures2/status.md` 登记；`civil_clock.rs`、belief、报表、plans、publications、session 中都是 `#[test]` 场景/辅助状态。未看到产品算法或 A 股制度参数进入本批变更 | 边界表述诚实；报告不是产品实现验收 |
| 同 16–42 | 逐动作 owner、调用、原断言、非法存档不经合法 fixture 修复 | 报告引用的源位置与 fixture2 status 动作相符。`TestOrderSaveFixture::restore` 消费单个合法 SaveSlot；坏档测试仍可在测试中编辑独立 SaveSlot 后直调 `GameSession::restore`。迁移 fixture 不改变公开存档入口 | 旧结论成立；静态代码路径保留坏档负向覆盖 |
| 同 44–59 | F1 注释修复、发行人具名输入、TradingPlan status 测试兼容迁移 | F1 当前说明为中文；发行人 kind/total_issued_shares 仍显式传递；status 修改只替换序列化 JSON 中对应字段。未见默认发行人或额外 PlanEvent | 修后复核未发现反证 |
| 同 61–71 | 190→191 测试名统计、只做 diff/rustfmt，无 Cargo/短测 | EOF 保留明确执行限制；未把实施者 rustfmt 当类型检查或 red/green | 可信的静态边界；不得据此报告这些 Rust 用例通过 |
| `review-performance.md` 1–21 | 六文件完整 diff、P2/P3 ownership 与 facade 发现，确认修后关闭 | 当前 `PerformanceComparisonRun` 把 `#measureSide`、`#appendSample` 私有化；`measure` 验 source before/after、validate 后才 append，且 `structuredClone`；`buildReport` 返回 clone；`runProcessSample` 仍为 async facade | P2-01、P2-02、P3-01 关闭状态与源码一致 |
| 同 23–39 | 三项门禁、样本单位/escrow 边界、ownership 范围 | 指标仍是 ticks/second、RSS bytes、线程观察；Resource decimal→BigInt 校验不引入 i64/u64 限制。未见撮合/结算实现变更 | 既有领域语义复核无反证；这不证明真实性能或 A 股规则本日重验 |
| 同 41–52 | sampler rejection child 清理问题、UI cleanup 顺序/失败截断、旧 reuse 不逐样本复验属于保留问题 | ProcessSampleRun 先等 child `close` 才 `await sampler`，sampler probe rejection 没有停止 child 的路径。`MarketUiReportRun.close` 顺序 await client、browser、server、profile；任一步失败都会跳过余项；POSIX stop 仅 SIGTERM | “不是本批修复”事实成立；按正式测试期限/资源收敛要求仍是候选，OOP 范围不能免责（见候选 C66-1） |
| 同 54–76 | 仅内存短复现、未跑 browser/真实性能；修后三项定向测试 7/7，报告结尾仍保留旧清理边界 | EOF 清楚区分实施者 35 个测试与 reviewer 修后 7 个测试，并未称真实 benchmark。当前报告仍把旧 sampler/UI cleanup 明确排除在修复之外 | 旧结论证据边界准确；但“保留”不等于满足当前正式验收约束 |
| `review-production-entry-caller.md` 1–7 | 全文 caller、三 getter、两行迁移、基线与领域依据 | `TradingPlan.account()`、`filled_qty()`、`active_child_order_id()` 都直接返回原 Copy 字段；example caller 只将字段访问换为 getter。`filled_qty` 注释明确是累计真实成交股数 | 实际改动与报告相符 |
| 同 8–22 | 三门禁、事件分流/单位/计时/随机数不变、只做文本比较 | 调用条件仍是同账户且 `(active child 存在 || filled_qty > 0)`，没有把子单受理冒充成交；输出、计时、订单与 RNG 代码均非本 diff | 旧结论成立；无新增 A 股语义候选，也未运行性能验收 |

## 原结论复核与依据

- **fixtures2**：报告按 A 股重构不变项核对 T+1、现金/股份预留、零股余量、披露口径及模拟休市时钟；涉及交易制度时沿用 `trading-rules.md` 已记官方核对日期，没有冒称本次重新访问交易所。当前 scope 是测试夹具重组，未发现领域漂移或测试断言弱化的源码反证。工作文件 `status.md` 后续记有 root 代表性验证，但本复核未独立重跑，不扩写为全套通过。
- **performance**：三项修复均在当前代码中存在，测试记录将初审与修后 7/7 分开；“复用报告未扩成逐样本重验证”也确实未被声称修复。既有 UI 清理截断与 sampler child 收敛问题是真实实现行为，不因报告将其排除在 OOP action 外而从 AGENTS 的进程树期限约束中消失。
- **production caller**：TradingPlan getter 属相邻封装的窄只读 API，caller 仍按账户身份、活动子单引用和真实成交进度计数；无计划受理/成交语义偷换。未发现交易规则变化，不需要为纯访问器迁移补查新制度条款。

## 新候选与反证

### C66-1：行情性能正式入口缺少进程外总期限与可保证收敛的清理

正式入口 `package.json:23` 是直接 `node scripts/performance/market-ui-report.mjs`；`scripts/performance/README.md:14` 将其作为性能回归运行命令。现行 `AGENTS.md` 对必要长验收规定每个 child 与进程树/临时文件清理共用 300000ms 上限，并要求正式命令由进程外 supervisor 看守。该入口没有 supervisor；源码 `waitFor` 仅有局部轮询期限，CDP pending、单次不 settle 的 fetch/probe 和 cleanup 不受统一 wall-clock deadline 约束。失败清理会在 browser stop 抛错时跳过 Vite/profile；POSIX stop 只发送 SIGTERM，不等待 child 退出。

反证与适用范围：性能命令有 1–60 秒观察窗口，且不是 `test:market-performance:unit` 普通短测试；README 没有明确它是否属于每次必跑的“长验收”。因此本项作为**正式入口是否列必要长验收需总审计确认**的候选，不宣称产品功能故障，不据此要求恢复某种 OOP 结构。若它是项目必要性能验收，则当前入口不满足期限/清理约束；若不属于必要验收，需在正式验收分类中明确，而不能让其作为无期限必测入口。

### 排除：fixture 同步与 getter 访问器

候选“fixture 合法化吞掉坏档失败”被 `session.rs` 坏档测试仍直接编辑/恢复的路径反证；候选“active child 被当成真实成交”被 caller 原 OR 条件与 `filled_qty` 字段定义反证。两者无新发现。

## 最终判定

三份审查文档均逐行覆盖至 EOF。原报告关于 ownership/facade 修复关闭、caller 访问迁移等结论与 `08e4fc7` 当前源码一致；未发现 A 股交易语义漂移。保留 C66-1 供总审计依据正式验收分类决定是否接纳，另记录 sampler/UI cleanup 是实际未修复资源收敛边界。没有修改产品代码、Git 状态或运行测试。
