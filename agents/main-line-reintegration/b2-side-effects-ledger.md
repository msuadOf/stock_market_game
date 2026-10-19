# B2 批副作用核对台账与实现登记

> 工作文件（agents 台账规范）。实施：feat/reintegration-b2-query-api
> （c61e8c68 → ed56b074 共 6 提交）。基线对照：18725d07 session 级契约。

## 副作用核对清单（验收 7——逐查询对照）

| 查询面 | 18725d07 基线 | B2 实现 | 一致性 |
|---|---|---|---|
| `query_stock_history` | 记 history_reads（账户+code+市场分钟） | 未改动（在树原样） | ✓ |
| `market_history_page` | 纯查询不记 | 不记 | ✓（测试锚定） |
| `query_market_history_for` | 记（恰请求账户一股；无效请求不记） | 同 | ✓（测试锚定） |
| `current_minute_history` | 纯查询不记 | 不记 | ✓（测试锚定） |
| `query_current_minute_history_for` | 记 | 记 | ✓（测试锚定） |
| `query_personal_trade_history` | 纯查询不记、不改日志 | 同 | ✓（测试锚定） |
| `query_intraday_average` | 纯读 | 同 | ✓（无写路径） |

## 语义差异登记（对照旧基线）

1. **receipt_id 空间**：旧=订单粒度 Fill 回执（envelope ledger 索引，
   稀疏）；新=成交腿粒度 `trade_seq×2+leg`（Sol 裁决——确定性/全局唯一/
   免新号段）。语义等价面=每腿每账户一条含费用三分项；id 空间不同。
2. **watermark 独立**：`receipt_watermark`（B2 新字段）不复用
   `next_receipt_base`（envelope ledger 游标——其一致性校验绑定旧域，
   挪用破坏 ledger 对账，restore fault2202 实证）。
3. **DeskDrained 协议扩字段** `drained_minute_bars`（日结/期末 Drain
   应答捎带当日桶——roll_day 先于 Drain 的生产时序由 desk 跨日暂存
   `rolled_day_bars` 兜住）。
4. **DeskTradeFact 扩** `maker_fee_split`/`taker_fee_split`（合计字段
   保留——守恒审计口径不变）。

## main 既有漂移登记（B2 发现、未越界修——交复核裁决）

1. **日界拍占位日K**：desk roll_day（tick=tpd 首拍开头）先于 session
   日终迁移——发布版本 active 已 roll 为 None ⇒ install 合成占位 K
   （volume=0）提交；真实当日 K 只在 desk.candles 观察面。主区探针
   （main HEAD）实证 vol=0。B2 冻结对账在占位形态跳过逐位（注释锚定
   drive.rs）；部分丢失形态（candle=100 bars=200）由测试构造规避
   （日界拍成交）。**建议独立批次修日界编排**（涉 install_candles
   相位状态机/迁移时序）。
2. save_contract 5 例失败（bank_policy/restore_guards/failures）——
   main 即红（两侧清单逐名一致），非 B2 引入。

## 性能门（验收 5——B 探针配对，main vs 分支，各 3 轮中位）

| 负载 | main | B2 初版 | B2 收口 | 判定 |
|---|---|---|---|---|
| 1000 tick | 932 t/s | 853（−8.5%） | **926（−0.6%）** | 持平 |
| 300 tick | 695（噪声 607-876） | — | 797（稳定） | 噪声主导 |

- 初版劣化归因：install 段每 tick 全量重建镜像（双拷贝 ~100μs/tick，
  量级吻合）；修复=增量追加（`runtime_append_minute_bars`）。
- 闭环：负结果 → 归因 → 修复 → 复测持平（CLAUDE.md 性能闭环走完）。
- 语义等价：成交量/费用桶/T+1 冻结与 main **逐位一致**（300/1000 全部
  轮次）。

## 其余验收对照

- wasm：`cargo check --target wasm32-unknown-unknown` 0 错（串行管线
  不经 runtime——查询面 wasm 下读 session 镜像（恒空）+civil clock，
  保留历史面数据缺失属 W3/串行批范围）。
- tsc：32=main 基线 32，不扩大。
- ts 生成物：~15 新类型随 export_bindings 落盘（SaveSlot 扩
  market_minute_bars/retained_market_days/personal_trade_confirmations/
  receipt_watermark）。
- 发布版本滞后显式失败：既有面（collect_resting_facts/install_candles
   等 `published.tick != committed` 显式 Err）不动；**查询面走 session
  镜像**（install/日结安装的发布投影，步间一致——无滞后面）。
- save/restore roundtrip 三面：活动分钟K（market_minute_bars 位置合并
  +DeskMarketSeed）/保留链（SharedRetainedDay serde 委托内值）/receipt
  日志（Vec 平铺+watermark）——测试锚定（retained_snapshot_tests/
  personal_receipt_tests）。

## 独立复核处置（复核 agent 报告全量收口）

- **阻断 1（已修）**：`market_minute_bars` 补进
  `RuntimeInstallBaseline`+`rollback_runtime_step`（整值回滚源）；回归
  测试 `install_failure_rolls_back_bars_and_retry_does_not_ghost`（注入
  安装失败→毒化终态+镜像零 ghost）。注：安装失败在既有设计=毒化终态
  （v0 投影失败才走 driver 丢弃+from_session 重建的可重试路径）——
  ghost 播回由"本回滚+重建 seed 等价"共同阻断。
- **建议 1（登记）**：收盘竞价成交 receipt 的 civil_date 取事件组装
  时点（drain 事实迟到下一拍 DeskReport）→ civil_date=次日，与 bars
  归当日 900 矛盾；日结步后立即 save/restore 该成交 receipt 丢失
  （DeskMarketSeed 无 trade_facts）。**事件域 drain 迟滞系既有**
  （drained_lifecycle/auction 同病），receipt 面新暴露——建议与日界
  编排批次合并修（Drain 应答扩 trades 或 receipt 累积移 drain 段）。
- **建议 2（登记+安全论证）**：end_civil_day 补冻结失败点在 assemble
  （push 前）——失败无半状态；world.freeze 后 session.push 不可失败。
  已在冻结入口加同日幂等（复核建议 3 一并覆盖）。
- **建议 3（已修）**：`freeze_retained_trading_day` 开头同日 already
  检查（手动 day_end 二次调用不再把 day-1 bars 冻入 day-2）。
- **建议 4（已修部分）**：DBG println 删除；cfg(test) 三重堆叠与契约
  测试非 runtime 跳过=仓库既有测试形态（sol 系同式；默认 env
  ENGINE_STEP_RUNTIME=true 下不空过），不改。
- **建议 5（澄清）**：`run_price_volume_baseline` 门未动——复核 diff
  基线取到 18725d07（多合并基线误判）；对 0d9f15a1 的本批 diff 仅
  fees mod+import 排序。
- **建议 6（已修）**：TapMessage 死代码删除（invalid() 直达 1261 码）。
- **建议 7（已修）**：u128 成交额 serde 边界用例重立
  （minute_bar_wire_tests——4×5e18 分=2e19 超 u64，roundtrip+非规范拒）。
- **登记确认**：旧档四新字段仅 receipt_watermark 带 serde default
  （其余三无——**旧档整体本就不兼容**（无版本政策，memory 登记），
  default 仅为过渡便利，一致性以批 9 存档清算为准）。

## 复核增量（第二轮）处置

- **① 失实申报更正并补实**：9cb8e110 提交时 `freeze_retained_trading_day`
  的同日幂等检查被后续编辑覆盖未入树（commit/台账申报与代码不符——
  复核抓实）。本轮已补真实实现（drive.rs 函数头 already 检查）。
- **② 补强测试**：新增 `candidate_projection_failure_rebuild_is_ghost_free`
  （v0 同式投影回调失败→镜像回滚+**重建输入面**（runtime_market_seeds 与
  save）零 ghost 锚定）。端到端重试链（driver 丢弃→from_session 重建→
  重试成交）卡在 seed 单与信封账本交互（重建 adopt 校验），随 W3 接线
  批补——登记。
- **③ 建议 2 登记补实**：end_civil_day 补冻结失败的半状态面——assemble
  失败无 retained 半状态 ✓，但 civil clock 已在 with_prepared_civil_day
  推进，Err 后重调 end_civil_day 会双推进（CivilClock 重复日结校验拦截
  ——显式失败非静默，但宿主需丢弃该会话态重建）。如实登记。
- 复核方撤回建议 5（基线误判）——已确认。
