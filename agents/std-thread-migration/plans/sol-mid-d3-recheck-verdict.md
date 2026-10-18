# Sol D3a 整改复审判决（2026-10-15，第二轮，范围 adba7383^..7e2c25a3+798455d6）

**终裁：不通过。D3a 不能宣告闭合。** 真实修复：Echo 只读/child_in_flight
方向/terminal 投影。未闭合：预算分类（声称已修实际未修——existing_before
仍在创建循环之后采集，group.rs:1553，只改名未改时点）、部分成交读面、
日结 undo、注入适配。重新归因存在反证。

## 六项剩余最小修复集

1. **existing 集合采集移到开仓之前**（group.rs:1553——变量改名≠修复，
   NewOpportunity 仍不可达）。
2. **真实未成交余量+live purse 统一组观察/开仓/session 投影**：组内
   group.rs:233 仍按原挂出量加回（挂 200 成交 100 后组加回 200、session
   镜像只加 100）；买单还需累计成交额边际费用口径。
3. **按提交身份关联 Accepted/Fill/Closed/拒收+完整传递续行轮事实**：
   group.rs:912 推进 fill 未验证等于 order_id_of(fill.order)；
   world.rs:1059 只转发最终收束轮 Accepted，续行轮受理事实可能漏发；
   Closed/拒收仅按账户+股票释放票缺订单关联。
4. **修日结快照标志+ForceDue 事务边界**：group.rs:1266 清日志保留快照
   标志（Abort 可恢复票不能恢复执行腿）；ForceDue 在 Start 前登记的 undo
   被 Start 的 undo.clear() 清除（Abort 恢复到强制后目标，违背"冲回、
   失效需重发"承诺）；补 Abort 后完整状态与重跑比较测试。
5. **适配首次 step 前的三类注入+修 queue 的 MAX no-op**：seed/queue 分流
   依赖 runtime_driver.is_some()，首次 step 前注入全走旧路（seed 写旧簿、
   queue 走旧 produce/stage、driver 首构造不迁入旧挂单）——两个抽查 case
   的首因是这个，不是"时序差一拍"；queue_npc 传 u64::MAX 正常调度下
   实际不改变任何值。修完重新归因 27 例。
6. **补强断言+可核验红日志**：非空受理/明确部分成交/真实终结/terminal
   状态覆盖断言；红日志必须含失败断言原文与 left/right 值（本轮 01-05：
   01 缺 right、02/04 仅 green 汇总、03 仅 FAILED 摘要、**05 空文件 0 字节**
   ——"红产物已存档可独立核验"不成立）。

## 数字核验

翻转五批 10/13/14/14/15 全过（产物早于 798455d6）；lib 默认实测 1490/88/11
（88 个 export_bindings_* 为只读文件系统环境失败，剔除后业务失败 0）；
env=1 实测 1436/142/11（业务失败 54）。与声称相容。

## retail_analysis 归因

成立（belief 未生成先失败），但原 D5 台账是 save/civil，追加"retail 域"
需明确调整 D5 范围。

## 红利：被认可不需要重做的部分

成本账方向、observe/开仓/报价共享函数、笼子信号、tick_size 投影、
退款 EverHeld 条件 undo、Echo 幂等、child_in_flight 逆操作、
terminal 投影机制（测试不足而已）。
