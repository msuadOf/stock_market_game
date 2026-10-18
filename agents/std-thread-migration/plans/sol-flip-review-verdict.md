# Sol 翻转批复核终裁（2026-10-15，gpt-6.1-sol high，独立复核 76c26e22^..62ff4c9b）

**终裁：当前不通过，不能宣告本批完成。** 默认回退旧 pipeline 的决定维持正确；
"50 项迁移等价完成"与"106 项失败全部属旧测试机制差异"两个结论被推翻。

复跑证据：runtime_flip_batch1-5 52/52 通过；runtime 94 项整组复跑触发 10s deadline
未取得完整结果；未重跑 lib 全量与性能。默认路径通过、`ENGINE_STEP_RUNTIME=1` 失败的
进程级对照 case：`session::failure_tests::public_step_rejects_and_poisons_a_missing_npc_strategy`
（预期错误却得到 `Ok([])`）。

## P1 阻断（必须修复）

1. **拍卖双撤资源闭合缺失**：`packages/engine/src/runtime/desk.rs:1439` 同 owner 分支只调
   两次 `remove_order`，无 `close_order_entities`（不退款退股）、无生命周期终结事实。
   两张交叉自单从簿删除但 purse/余股/在途闭合标记未返回。
2. **拍卖成交量上报失真**：`desk.rs:1395` 实际配对量允许小于选择量后，
   `AuctionCompleted.matched_volume` 仍上报 `sel.volume`。反例：上报成交 100 股、实际零 Trade。
   内部选择量可作上界，公开成交量必须取实际成交量。
3. **Inserted 回放漏索引同步**：`desk.rs:421` `remove_by_ref` 只删簿单不减
   `self_bids/self_asks`。反例：插入 A 卖单→Abort→A 买第三方卖单，被不存在的自单阻断。
4. **turnover_today 未入审计面**：未进 `MarketSnapshot`/rollback/`state_hash`。
   有成交后 Abort，失败 tick 成交额残留到活动 K，回滚哈希检测不到。
5. **配置硬编码**：`drive.rs:279` `ticks_per_day/auction_ticks/closing_auction_ticks/history_len`
   写死 `240/15/3/16`，非默认配置产生错误投影。必须从真实配置产生投影。
6. **第二日分钟投影日偏移双计**：`desk.rs:1637` 绝对分钟数当日内 Vec 长度、安装再加日偏移。
   默认配置第二日 tick 255、连续分钟为零却填出 240 条分钟记录。
7. **日结失败原子性**：`drive.rs:179` 在可失败的 `end_civil_day()` 前推进 session day
   并分配清簿事件 seq，失败时不恢复。
8. **opt-in 入口真实缺陷**：`failure.rs:36` 每步重建 World，构造器不恢复市场 tick/订单簿/
   策略状态（World 每次执行 tick 1，session tick 持续增加）；两个安装方法 Result 被忽略，
   步进失败仍执行安装；`runtime_flip_batch5.rs:138` 的 opt-in 测试只跑 `--list` 没跑 `step()`。
   修复或暂时禁止不完整入口正常成功返回，停止吞安装错误。
9. **无效测试断言**：`runtime_flip_batch3.rs:59` `assert!(条件 || true)`；无符号 `>= 0` 恒真；
   只计数不断言的拒单测试；T+1 只查现金非负；清簿闭环只查 `accepted >= canceled`。
   全部替换为真实断言（保留原语义强度，不许放宽）。
10. **attention candidate 假迁移**：`failure.rs:209` 过期 candidate 直接推到当前 tick，
    未执行观察/RNG/调度状态演进——是修补校验条件不是真实状态迁移。

## 维持的裁决

- **回退 opt-in**：维持（保护默认生产路径）。
- **日终显式 OrderCanceled**：方向维持（A2），但实现验收未过（须验证准确订单集合/
  余量/退款退股/事件恰一次及 seq；加第 7 条原子性）。
- **连续竞价"终止撮合"**：会阻断本可先成交的第三方（同价档 B 先到卖单、A 后到，
  A 买入因该档存在 A 自单直接停止，B 的先到卖单不成交）。必须登记为**游戏额外限制**，
  删除未证明的"与旧管线等价"结论。
- **条款依据**：深交所"2025 年修订"与官方来源"2026 年修订"版本矛盾；网络受限未能核验
  原文。修订版本号与生效日期，核验前不宣称依据可靠。

## 修复后要求

1. 逐项修复 P1，红→绿测试先行（资源闭合/索引/回滚/原子性各自带反例测试）。
2. 106 失败盘点结论作废，P1-8 修复后重新逐项分类（至少一条已证实是真实语义遗漏）。
3. 官方依据修订后再次独立复核（Sol），通过前不宣告翻转批完成。
