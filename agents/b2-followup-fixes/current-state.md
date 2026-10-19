# B2 批补面修复台账（W3 接线实证两缺陷）

分支 `fix/b2-followup-wasm-stats`（worktree `.worktree/reint-b2fix`，基线 main=fe9e2ff5）。

## 缺陷与修法

### 缺陷 2（提交 6894d8c3）：占位 K 覆盖真实 stats

- 现象（W3 worktree 实测 wire）：真实跨价成交日结后
  `daily_candle = {OHLC:900 真实, volume:0, turnover:0, trade_count:0}`，
  而分钟桶=100 股/90000 分/1 笔。
- 根因：日界拍 desk `roll_day` 归档真实 K 后 `snapshot_market` 每拍
  `get_or_insert` 零占位 candle ⇒ 发布版本 `view.daily_active=Some` 但
  `v=0`；`install_candles` 的占位路径只识别 `None` 一种形态，active
  装载分支以零量值整体覆盖 session 真实活动 K，日终迁移把被覆盖的 K
  归档（`assemble` 对账的"占位放行"分支曾登记该漂移）。
- 修复：占位判别扩两源（`None` 或 `v==0`）；session 已有当日活动 K
  （time 匹配）时跳过安装——占位只补 OHLC 缺失场景。
- 红证：`retained_snapshot_tests::traded_day_daily_candle_stats_match_minute_bars`
  （红：日 K 量 0 vs 桶 100 → 绿）。
- 附带：`candle_knob1` 判别测试换真实成交前提（零量场景不再走 active
  分支；与 knob2 同款 inject，判别面更强）。

### 缺陷 1（提交 8a9824f9）：wasm 串行路径不产 B2 数据面

- 现象：`step_runtime_enabled()` wasm32 恒 false ⇒ web 走串行旧管线，
  B2 三安装面（分钟桶/receipts/retained 冻结）全在 runtime 路径 ⇒
  wasm 四查询面恒空。
- 修复合同（单一事实源，不为 wasm 写第二套聚合逻辑）：
  - desk 分钟桶聚合/键推导提取为 `runtime/protocol.rs` 纯函数
    （`minute_bar_apply`/`continuous_minute_of_day`/`OPEN|CLOSING_AUCTION_BAR_MINUTE`），
    desk `apply_trade` 改调同函数（行为等价重构）；
  - retained 组装核心挪 `GameSession::assemble_retained_day_snapshot`
    （driver 编排委托同函数）；
  - 新模块 `session/pipeline/serial_b2_surfaces.rs` 接线
    `finalize_continuous_tick`：每拍 Trade 事件聚合桶 +
    `ReceiptBatch` 逐笔双腿 Fill 收据入账（费用三分项=旧管线 envelope
    实收权威；receipt_id=Fill 收据 ledger 全局索引，单调续号）；日结拍
    迁移前 take 桶、迁移后冻结；
  - `commit_from` 补三 B2 派生域交接（此前只解构不赋回——candidate
    写入在 commit 边界丢弃；Sol 五轮 desk_book_mirror 同款形态）；
  - `end_civil_day` 串行分支补休市/未冻结日条目；
  - `executor_perturbation_tests` fixture 开局改 2030-01-02（原
    2030-01-01 元旦休市——旧管线可休市撮合但 B2 日历权威判定休市=
    Closed/空桶，冻结显式失败；该批测试断言并发不变式不涉日历）。
- 红证：`serial_b2_surface_tests::serial_pipeline_produces_all_four_query_surfaces`
  （ENGINE_STEP_RUNTIME=0 子进程探针——真串行分叉；红：分钟桶空 →
  绿：四面全出数）。
- W3 实证等价：`w3_query_surfaces_equivalent_real_traded_data`（W3
  protocol_tests 断言集的引擎直达版——ProtocolSession 查询委托属 W3
  接线批）。

## 验证矩阵（提交后树）

| 项 | 结果 |
|---|---|
| engine lib 全量（缺省 runtime） | 1776 passed / 0 failed / 11 ignored |
| engine lib 全量（ENGINE_STEP_RUNTIME=0） | 1775/1——唯一失败 `memberships_tests::members_trade_in_one_shared_book...` 为 **main 基线串行同败**（既有，与本次无关；对照实测） |
| wasm32 check（engine / web-wasm） | 0 error |
| web-wasm lib（native） | 18 passed / 0 failed |
| 翻转批集成 batch1-5 | 全绿 |
| npc_decisions/skeleton 偶发 | 全量并发下轮换偶发（单跑恒绿、main 同形），非本次引入 |

## 复核记录

独立复核（未实施 agent，两轮）结论：三笔提交（6894d8c3、8a9824f9、
258b0271——第三笔=复核两 MINOR+NIT 修复）合并门禁通过，无
BLOCKER/MAJOR/MINOR；红证强度经基线独立验证。详见 review.md。

## 遗留（后续批，NIT 级）

- receipt_id 跨管线两 id 空间升序混载静默通过（单调校验仅拒降序）——
  已在 serial_b2_surfaces 模块注释登记。
- serial 一拍多价/同单多 Fill 专项测试缺（聚合函数本身有 runtime 侧
  wire tests 覆盖）。

