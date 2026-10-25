# #116 全场休市门禁与交易 fixture 迁移

## 范围与状态

- 本批从 `f58202911b8db6c042fe1816d08277c16381f2d2` 新建独占 `fix/finish-116-holiday`，原 `.worktree/holiday-gate` 的 dirty/index 只读且未改。
- `check_step_start` 在既有前置检查后，用 `CivilClock::calendar()` 逐股查询本局冻结政策；全场休市时，在意图出队、RNG、撮合或行情提交前显式拒绝。沿用 `StepFatal` 的 poison 契约，未新增可恢复错误语义。
- `runtime_stock_trading_on` 改读同一本局政策，避免日终保留链与市场入口对同日给出不同结果。
- `CalendarPolicy::default_start` 及产品默认元旦开局不改。正常休市日日结仍处理经营和披露并进入下一自然日。
- **本批不是 #116 全部关闭**：混合日历逐股隔离仍为后续交付，不新增 Unsupported 产品决定。未实施者独立终审由父 agent 安排。

## fixture 精确迁移清单

| 位置 | 迁移原因与边界 |
|---|---|
| `src/session.rs::npc_working_quote_tests::quote_setup` | 同拍申报、报价、受理/撮合、回滚和存档测试继承此 helper；首拍必须是交易日，由元旦改为 1 月 2 日。`gap_stock_setup` 已走双时钟，保持不改。 |
| `src/verification_evidence/phase_timing_tests.rs::session_with_institutions` | 证据针对实际市场 tick；只迁该 helper。 |
| `src/session/protocol/civil/session.rs` 的 5 个 rollback 用例 | 仅这 5 例需要立刻 step，局部覆盖为 1 月 2 日。共用 `publication_tests::setup` 和 closed-day 专测仍保留元旦。 |
| `tests/session.rs::sample_setup` / `TestOrderSaveFixture` | 本套件要求直接成交；交易 helper 改 1 月 2 日，并提供显式日期入口；新休市/日结测试仍显式设 1 月 1 日。双时钟推进断言保留。 |
| `tests/auction.rs::AuctionFixture::{quiet,representative}` | 集合竞价 fixture 必须开市，分别迁移两处。 |
| `tests/diagnostic_absence.rs::setup`、`tests/diagnostics.rs::setup` | 诊断开关与事件链要求实际 step；仅迁开局日期。后者受预存 feature 编译错误阻断，未声称特性验证通过。 |
| `tests/attention_discovery/failures/discovery.rs::discovery_setup` | 私有信息隔离比较真实订单链，迁其交易起点。 |

未盲替会计事实日期、披露窗口、政策 manifest、默认开局、自然日和休市专测。没有删除或弱化断言。

## 红绿与原件

- 新增 6 例：市场 step 有对手盘休市拒绝且开市真实成交、显式 runtime 入口原子性、正常休市日日结后可交易、frame 与 evidence frame 门禁、沪深各自冻结闭市、冻结政策逐股查询。
- `red-final.log`：旧生产实现下 5 个业务失败，1 个自然日日结原本通过。冻结 gate 在合法 SSE 首轮已真实产生 PriceTick，因此红证有效；当时 SZSE 轮尚未执行，后续为其纠正股票代码后才验证绿，不声称原红覆盖了两轮。
- `green-final.log`：最终 6/6 通过，含沪深两轮及 StepFatal poison 断言。
- 原始红失败尝试 `agents/branch-completion/116/red/`、`red-corrected/` 存在 InvalidSave 2066/1038：分别为全年覆盖改变前史、直接克隆手工挂单 save 未经 helper 收据同步；**不能作为对应冻结查询的 TDD 红证**。后改为无手工挂单的真实 save，`red-final` 在成功 restore 后错误开市断言变红。
- 最初绿失败 `green-final/` 的 SZSE 股票仍用沪码，产生 InvalidSetup 1021；已改合法 `000101` 并在 `green-final2` / `green-binding` 验证通过。全部旧日志保留在主仓 `agents/branch-completion/116/`。
- 门禁前目标库套件 1222/1222 通过；仅加门禁后 82 失败；迁移后 1222/1222。最终相关普通验证共 1445 例通过：库 1222、session 144、auction 32、attention 20、absence 2、civil 13、frame 12。不是完整全仓回归。

## 命令、资源与绑定

- 独占构建目录：主仓 `.tmp/finish-116/target`；构建 `CARGO_PROFILE_DEV_DEBUG=0`、`-j 32`、offline。使用主仓 `node scripts/run-long-validation.mjs 300000 -- cargo test --manifest-path .worktree/finish-116-holiday/Cargo.toml -p engine ... --no-run --offline -j 32 --message-format=json`。
- 普通用例通过主仓 `agents/branch-completion/116/run-cases.py` 按 binary/case 隔离进程，16 个 case 并发；单 case 外部进程树 10000ms，整批由 `run-long-validation.mjs 300000` 外部 supervisor 限制。没有启动完整 all 批次。
- 初次构建 `build-red*`、最终 `build-final*`、复核构建 `build-binding*` 的 stdout/stderr 全部保留在主仓 `.tmp/finish-116/`。运行期间核对两条 engine rustc 并行、各约 100% CPU；rustc 单个编译单元的限制未伪称多核撮合测量。
- 最终绑定构建前记录 `.tmp/finish-116/binding-source-before.json` 的所有 engine/Cargo/config 文件 SHA256，构建后确认源未变；`binding.json` 给出实际 binary SHA256 与 mtime。engine/session/civil/frame 与先前通过的 binary hash 一致；auction/attention/absence 链接产物改变后已对新 binary 重跑，不借用旧结果。
- `regression-summary.json` 保存实际 case 计数、用时、失败列表。源文件全集 manifest 原件及 Cargo JSON 产物路径均在 `.tmp/finish-116/`；本地 commit 冻结最终 diff 供独立审查。

## 诊断 feature 未通过的边界

`--features simulation-diagnostics --test diagnostics --no-run` 因基线 4 个生产类型错误失败：`continuous_tick_finalizer.rs:291` 缺少 TradingPhase，`causal.rs:85` 的 issuer_of 参数与 Option.cloned，`notices.rs:397` CompanyId 借用类型。原件为 `.tmp/finish-116/build-diagnostics.{jsonl,stderr}`。父 agent 已独立分派修复，本批不顺带改这 3 个生产文件。

## 混合日历后续输入与分层

可恢复输入：2030-01-02 开局、沪深两股，SSE 的 2030 OfficialCoverage 仅将 1 月 1–2 日设为闭市，SZSE 留模拟回退。覆盖必须含元旦，否则整年 Official 替代回退会改变 360 根前史。SSE 当日闭市而 SZSE 开市，不能因首股闭市暂停全场，也不能让闭市股成交。最小后续需独立处理 CivilClock 多所时钟、runtime/串行执行逐股过滤、每股分钟/日界及恢复一致性，不能用全局 Unsupported 降低已承诺语义。历史实现定位 `ce5b3a1c:packages/engine/src/session/exchange_calendar_tests.rs`，仅作设计线索，不把旧分支结果当当前验收。
