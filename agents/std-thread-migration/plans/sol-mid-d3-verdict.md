# Sol D3a 中点复核终裁（2026-10-15，gpt-6.1-sol high，范围 6a5ed768^..c903f63e）

**终裁：不通过。D3a 有真实移植成果，但尚不能宣告闭合；仅补注入通道不足以放行 D3b。**
执行腿、Abort 回滚及读面投影存在实质缺陷，独立 subagent 交叉确认。

## 八项关键阻断

1. **EchoOpenDerivation 实际可写计划**（group.rs:1208）：write_back=false 只跳过
   undo 快照，随后仍无条件 `self.plan_book.create()`——读面可能创建真实计划，
   第二次读取因 active 分流产生不同结果，违反只读/幂等/可逆承诺。
2. **child_in_flight 的 undo 方向反了**（group.rs:556）：插入记 inserted=true、
   rollback 却再 insert；删除记 false、rollback 再 remove。首次挂单 Abort 后
   残留在途位，重跑跳过报价。
3. **PlanBook undo 没有覆盖既有计划**：快照只在新建前保存；
   install_execution/record_child_submitted/record_child_fill/
   next_child_order_id 增量均缺回滚保护。state_hash 也未含这些新增状态。
4. **首笔部分成交被当成子单全成**（group.rs:711）：首见成交用 fill.qty 登记
   子单总量，紧接着以同数量 fill，remaining 当场归零、在途位清除；原单余量
   留簿，下次 due 重复挂单。FillFact.order 未参与身份匹配而另造 OrderId。
   "adopt qty 弱化"登记不成立，属阻断。
5. **自动到期/日终清簿已有场景，腿却不清理**：desk 已有 expire_due 与日终
   Drain；组侧 Closed 仅减 foreign_active，不清 child_in_flight/执行腿。
   未成交子单跨日后可能永久卡住。"未接 Cancel 前无场景"登记不成立。
6. **可用资产被当成总权益**（group.rs:1869）：组 cash/shares 已扣转交 desk
   的在途资源，却直接用于 observe 权益记忆和开仓目标——挂单即降所见权益、
   改持仓权重与目标量。不能整体推迟到 D3b。
7. **预算新旧计划分类漂移**（group.rs:1292）：existing 在创建完成后从全部
   active IDs 构造，新计划全部成为 ExistingPlan，NewOpportunity 不可达。
   应使用本轮开仓前的计划集合。
8. **最后一个计划结束后投影保留旧 Active**（drive.rs:391）：driver 只在
   active IDs 非空时安装组簿；最后计划完成/终止后 terminal 事实无法覆盖
   session 旧镜像。

## decision_chain 27 归因裁定

注入缺失**成立**；"27 例首因都是 due 无下行"**不成立**。抽查三例：
- 两例在"子单必须有在途身份"失败（执行腿首笔 fill 才登记身份——阻断 4）
- 一例 reference_equity=None（session 个人态缺生产交付投影——独立原因）

三种注入并不相同：force_attention 改 session 调度态；seed_order 写旧 book；
queue_npc 生产并暂存旧管线产物。一个"强制到期"指令不能替代三者。

**会话→组单向下行方向可采用**，条件：
- 测试控制面放 `cfg(test)`，显式指令表达账户+目标 tick，组线程验证并执行；
  不得每 tick 偷偷用 session 镜像覆盖组态
- force_due / book fixture / 计划根+个人态 fixture 分别适配，保持原测试意图
- 生产订单走正式 ingress/受理事实；测试不能直接改组内部字段
- Abort 后指令的恢复或重放语义必须确定；组仍是调度与计划状态权威

## 登记差异逐项裁定

| 项 | 裁定 |
|---|---|
| exposed=空集 | **须收口**（生产发现路径丢披露曝光权重） |
| 首子单 initial urgency/pause=Unavailable | 可接受，登记至 D3b 必须收口 |
| frozen_cash=0 | 可接受（预算 cash 已扣在途，再减=重复扣；不豁免总权益读面缺失） |
| qty 口径差 | **须修**（影响成本价/权益观察/开仓目标） |
| fundamental_ready 三处复制 | 可接受维护债 |
| adopt qty 弱化 | **须修·阻断** |
| 4b 红证自参照 | **须修·阻断证据闭合** |
| DeskTradeFact maker/taker | 可接受既有问题登记 |
| 撤销/到期腿清理 | **须修·阻断**（生产场景已存在） |
| 成交场景红证 fixture 缺 | **须补·阻断** |

## TDD 证据链裁定

成本账/observe/开仓核心/报价核心共享函数=有效移植；笼子信号/tick_size 投影/
退款 EverHeld 条件 undo 修复可确认。但"开仓逐字段同源"证据不足：
- 成本投影置零红等三处红证无原始红跑产物可独立核验
- 当前测试已移除 PlanOpen 字段对拍
- 4b 用实际受理 qty 反构造 allocation_cash（期待值受被测输出影响）；
  允许"仅有计划零受理"通过前置断言，报价循环可零次执行
  （drive_tests.rs:703/782）

## 数字核验

D3a 定向测试 8/8；翻转批 10/13/14/14/15；三个 env=1 失败复现（两默认对照
通过）；baseline/after-2c/after-3 三份=同一 55 项名单；after-fix 有 56 项
（watchdog flake 相容）；1574/0/11 未从 HEAD 重建验证。

## 放行最小修复集

1. 修 Echo 只读性、child_in_flight 逆操作、所有计划/身份变更的 undo
2. 接通真实受理身份、完整子单数量、部分成交、终结事实；覆盖到期、
   日终清簿及失败释放
3. 收口含在途资产的持仓/总权益读面；修新旧计划预算分类
4. 权威状态控制的完整计划投影（含最后计划结束后的 terminal 事实）
5. 补曝光输入+独立预期短 fixture（部分成交/全成/终结/Abort 重跑/读面幂等）；
   再接分类后的注入面并重新归因 27 例

可继续 D3a 收尾与注入通道设计；**不得宣告 D3a 闭合，"55 持平"不得作为
D3b 放行依据**。
