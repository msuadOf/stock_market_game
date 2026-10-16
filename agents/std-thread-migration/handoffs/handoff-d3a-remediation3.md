# 交接:D3a 整改第三轮(第 8 任→第 9 任,2026-10-15)

## 1. 当前状态

- **HEAD**:`798455d6`(feat(engine): Sol 中点整改注入通道+个人态交付投影)。
  工作区**干净**(git status 无输出;.tmp 不入库)。
- 本会话 D3a 全程提交链:`6a5ed768`(成本基)→`2ca76d31`(组装/直调)→
  `ccc58cb6`(开仓)→`e7814728`+`0d21521b`(投影)→`a27b0ad2`(复核1收口)→
  `3643ebaf`(4b 生产接线)→`f734cda4`(执行腿)→`c903f63e`(复核2收口)→
  `adba7383`/`3e59c462`/`e97146fb`/`15b6044b`/`7e2c25a3`(Sol 中点整改1-5)→
  `798455d6`(注入通道)。
- **Sol 二审裁决**:`.tmp/sol-mid-d3-recheck-verdict.md`——不通过,
  六项剩余修复(行号都在裁决里)。被认可不需重做:成本账方向、
  observe/开仓/报价共享函数、笼子信号、tick_size 投影、退款 EverHeld
  条件 undo、Echo 幂等、child_in_flight 逆操作、terminal 投影机制。

## 2. 六项剩余修复(Sol 裁决顺序,行号已核对)

1. **existing 采集时点**(group.rs:1553):`existing_before` 声称"本轮开仓前
   集合"但采集在开仓循环**之后**——只改名未改时点(Sol 点名属实)。
   修法:把 `existing_before` 的采集移到 `for (code, assessment) in &assessments`
   开仓循环之前(约 group.rs:1470 附近、`let mut opens` 声明处)。
2. **真实未成交余量+live purse**:group.rs 的 `held_qty_with_pending_at`/
   `cash_with_pending_at`/GroupAccount.pending_sell_qty 按票的**原挂出量**
   (ticket.qty)加回,部分成交后不跟随(挂 200 成 100:组加回 200、session
   只见 100)。修法:票的余量改用 `plan.active_child_remaining_qty()`
   (或票上记 remaining 并 fill 递减);买单 purse 还需累计成交额+费用
   的边际口径(裁决原话"买单还需累计成交额边际费用口径")。
3. **按提交身份关联事实**:group.rs:912 附近 `apply_fill_plan_progress`
   的 fill 推进未验证 `order_id == order_id_of(&fill.order)`(票已有身份
   时应校验一致,不一致=漂移显式失败);world.rs:1059 附近只在最终
   收束轮转发 Accepted——**续行轮(Continue round>0)的受理事实漏发**
   (要逐轮转发);Closed/拒收的 `release_child_ticket` 仅按账户+股票
   匹配票,缺订单关联(同账户同股多单时会放错——需要票身份比对)。
4. **日结快照标志+ForceDue 事务边界**:group.rs:1266 附近
   `handle_day_end` 开头 `self.undo.clear()` 但**没清 `plan_undo_saved`**
   (裁决:清日志保留快照标志——Abort 可恢复票但不能恢复执行腿);
   ForceDue 在 Start 之前入队处理时 push 的 NextDue undo 会被
   `handle_start` 的 `undo.clear()` 清掉(转正点清日志是既有语义,
   但 ForceDue 指令的效果应属于**下一 tick 的日志域**——裁决要求
   "Abort 恢复到强制前、失效需重发")。修完补 Abort 后完整状态与
   重跑比较的测试。
5. **首次 step 前的三类注入**:seed/queue 分流判
   `self.runtime_driver.is_some()`——**驱动器是首次 step 时才惰性构造的**
   (failure.rs:93 附近 get_or_insert 语义),测试在首次 step 前调
   seed_order/queue_npc 时驱动器还不在位→走旧路(seed 写旧簿、queue
   走旧 produce/stage)——**这是两个抽查 case 的真实首因,不是"时序差
   一拍"**(我上一轮归因错了,Sol 已纠正)。修法:分流入口改为
   "runtime 模式启用"(如 ENGINE_STEP_RUNTIME 或 will-step-runtime 判定)
   或分流前先确保驱动器构造。另外 `force_all_due_via_runtime` 传
   `u64::MAX` 被 `force_due_for_test_at` 的 `.max(world_next)` 钳到
   world_next——正常调度下 **no-op**(queue 语义没真承载),要改传
   world_next+1 或语义化目标。
6. **补强断言+红日志纪律**:受理非空/部分成交明确例/真实终结/terminal
   覆盖的断言都要有;**红日志必须含失败断言原文与 left/right 值**——
   本轮 .tmp/sol-mid-d3-red-evidence/ 的 01-05 不合格(见下),新红证
   用 `cargo test ... 2>&1 | tail -30` 落盘或手动跑一次把 panic 段
   完整贴入,禁用 grep 无匹配的空文件重定向。

## 3. 我踩过的坑/熟悉的代码位

- **group.rs 阻断位速查**:开仓消费+existing(1470-1560);执行腿
  apply_fill_plan_progress/release_child_ticket/adopt_child_on_accepted
  (700-900);在途读面自由函数 held_qty_with_pending_at/
  cash_with_pending_at/equity_cents_with_pending_at(文件底部);
  build_root_read_context/build_chain_observation(2200+);ForceDue/
  ChildAccepted 消息分支(group_main match 内,ForceDue 带 #[cfg(test)])。
- **借用陷阱**:GroupState 方法内 `self.accounts` 不可变借用期间
  `self.undo.push`/`self.plan_book` 可变操作会冲突——用块作用域先取
  数值再操作(closed ticket/adopt 段都有先例);`&mut self.decision`
  profiles 借用期间不能调 self 方法——run_round 里用 pending_next_due
  延迟入账的模式。
- **desk 语义**:DeskTradeFact 的 maker≡卖方/taker≡买方(两 taker_side
  下统一,desk.rs:1109 对调构造);价格笼子会拒收远离现价的限价单
  (测试注入用 last±1);玩家(AccountId(0))开局无持仓(卖单恒 skip,
  测试对手用 retail 位);卖出挂单即扣 shares(ever_held 置位)、
  成交事实在**下一 tick 的 Start 才落账组**(红证对拍前要补一个
  落账空转 tick)。
- **红证 fixture 心得**:belief 报价 Wait 的两大原因=EmptyBook(决策
  快照是上一收束点)和 protection disjoint(DeepValue 估值远离市价——
  正确保守语义);能出 Submit 的形态=auction_ticks=0+retail 对手位
  双向贴价注入(solmid_execution_leg_lifecycle 测试里有完整可抄的
  注入循环)。
- **world.rs**:受理转发在 run_ticks 的 install_tick 分支(1059 附近);
  续行轮(Continue)的 evidence 只在最终轮 install——续行轮 Accepted
  漏发是裁决 3 的一部分。

## 4. 声称过修复但未验证/未修完的清单(避坑用,如实)

- **existing_before=改名未修时点**(裁决 1)——最典型的失实,修法见上。
- **红证据 01-05 不合格**:01-red 是完整 panic(38KB,合格)但 01-green/
  02/04 只有 grep 汇总行、03 只有 FAILED 行、**05 是 0 字节空文件**
  (grep 无匹配 > 重定向的产物)。"红产物已存档可独立核验"不成立。
  下一任重做红证时按裁决 6 的格式落盘。
- **"时序差一拍"归因错误**:我归因 seed 场景失败为 external 单与决策
  同轮——真实首因是**首次 step 前驱动器不在位、注入全走旧路**(裁决
  5 的反证)。相关登记(台账 D3a-3 节)需要勘正。
- **pending_sell_qty 镜像的维护点**:fill 推进处减的是 fill.qty、release
  处减的是 plan remaining——但组读面加回的是 ticket.qty(原挂出量),
  两套口径不一致(裁决 2 的实质)。
- **env=1=54 的数字**:institution_observation 转绿真实;但我当时没跑
  第二轮确认 flake(历史上有 ±1)。

## 5. 注入三类适配已知状态

- force_attention 分流(session.rs tests mod 内 force_attention_candidate):
  已接 ForceDue——**但同样受"首次 step 前驱动器不在位"影响**
  (chain_restructure 的 seeded 场景 force 在 seed 之后、step 之前,
  驱动器已在(前面 probe/构造过)则生效;纯首 step 前调用则不生效)。
- queue_npc_for_next_tick 分流(npc_tick_preparation.rs:630 附近):
  已接 force_all_due_via_runtime——**u64::MAX 目标被钳成 no-op**(裁决 5),
  且首次 step 前同样失效。
- seed_order_for_test 分流(session.rs seed_order_in_book_for_test 头部):
  已接 D2 inject——首次 step 前失效;注入后 belief 决策快照仍无该单
  (同轮);seed 的事件缓冲在分流路径不产事件(测试若依赖 seed_events
  会空——没验证过有没有人依赖)。
- ChildAccepted 受理通道(生产通道):组内 adopt 已接;world 只转发最终
  收束轮(裁决 3 要求逐轮)。
- 个人态交付投影(driver step 尾 collect_group_belief→
  set_runtime_personal_state):已接,institution_observation 转绿的
  根修;每 step 一次 EchoBelief 的开销没评估(热路径成本登记缺失)。

## 6. 台账索引

- 总台账 `.tmp/multi-tick-design.md` 尾部:Sol 中点整改落地节+
  重新归因节(**归因部分需按裁决 5 勘正**)。
- 裁决:`.tmp/sol-mid-d3-verdict.md`(一审)+`.tmp/sol-mid-d3-recheck-verdict.md`(二审)。
- env=1 失败名单:`/tmp/env1-final.txt`(54 项,临时文件——重取用
  `ENGINE_STEP_RUNTIME=1 cargo test -p engine --lib | grep " FAILED"`)。
- 回归基线:默认 lib 1578/0/11(npc_decisions/watchdog 偶发 flake ±1-2);
  翻转批 10/13/14/14/15;env=1=54。
