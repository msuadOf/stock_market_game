# step_frame 接轨批·前置约束清单（Sol 四轮裁决登记，2026-10-18）

> 来源：sol-103 市价链路修复批（四轮）期间 Sol 复核发现。接通 Continue
> 到生产 Driver **之前**必须逐条处置，否则已修语义会以新形态回归。

## 1.（阻断级前置）Continue 早轮成交 vs drive 同拍投影的重复应用 ——【已收口，bcc18dbf】

- 事实：world tick 循环经 Sol 三轮修复后，Continue 链上早轮 desk_trades
  会累积进最终 TickRecord（正确——事实不丢）。
- 缺口：`drive.rs` step 段的同拍成交投影（`apply_same_tick_fill_projection`
  调用族，约 :714 起）以**本拍 desk_trades** 为输入做"组回写后叠加增量"
  ——Continue 接通后该输入将含早轮已落账成交，叠加口径会把早轮成交
  **重复应用**（现金/持仓双计）。
- 当前不可达性：生产 Driver 未接 Continue（continuation 恒 false），
  故现为潜伏缺口。
- 前置约束：接 Continue 前必须把同拍投影改为**逐轮**消费（每轮证据
  到达即投影一次）或按轮次去重；并补"Continue 早轮成交恰一次落账"
  端到端红测试。
- **收口（2026-10-18，bcc18dbf）**：按"轮次分离"落地——TickRecord 增
  `unsettled_trades`（终轮未落账尾巴），两个同拍叠加投影改消费尾巴；
  事件流/buy_vwap/裁剪保持全量。红绿证据
  `.tmp/sol-stepframe-red-evidence/01-prereq1-{red,green}.txt`
  （红=买方持仓 +200 双计；绿=恰 +100）。

## 2.（登记）driver-direct 存档不含 desk 序号游标

- desk_book_mirror（含 next_serial——save 游标权威源）在 session.step
  的 install 段构建；测试/协议宿主直驱（RuntimeDriver 直调不经
  session.step）时 save 的 book_next_sequences 走旧簿分支（游标 0）。
- 影响：仅测试面/非生产宿主；生产路径（session.step）已贯通
  （sol4r_serial_cursor_survives_save_restore_roundtrip 锚定）。
- 处置建议：直驱流程需要存档游标时先过一次 session.step 或显式调
  镜像刷新；长期可把游标随 DeskReport 每拍上行。

## 3.（登记）NPC 路径 Lowest 买 purse 仍按统一涨停（F §1.5 修订注记）

- 现金 ∈[跌停 purse, 涨停 purse) 时同一 Lowest 买单玩家受理、NPC 拒收
  （双侧可达性差异，已如实登记于 F §1.5）。
- 收口待办：decision.rs symbol 价 purse 改跌停口径需 MarketView 贯通
  daily_lower_limit（策略域公共类型扩字段）。
