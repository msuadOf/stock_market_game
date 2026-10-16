# Sol 整改复审终裁·第二轮（2026-10-15，gpt-6.1-sol high，复核提交 5412fd10）

**终裁：不通过。5412fd10 有真实修复，但"P1-1～P1-10 全清"不成立；翻转批仍不能宣告完成。**

裁定明细：P1-2/4/6 修复真实；P1-1 正常分支真实但验收未闭合；P1-3/5/8 部分真实；
P1-7 表面修复；P1-9 未修完（声明不实）；P1-10 有真实演进但迁移未闭合。
拍卖"撤卖侧保买侧"策略：**维持**（trading-rules.md 登记合格）；但代码/测试中
"等价双撤、资源全部退回"旧注释必须修正；官方条款版本仍标未经核验。

## 剩余最小修复集（六项，完成后再次独立复核）

1. **回放删除空价档**：`remove_by_ref` 不删空档（desk.rs:488），拍卖清算直接取
   `keys().next()/next_back()` 会踩空档提前结束零成交。反例：挂卖 900→Abort→
   重跑第三方卖 1000/买 1000，选价见有效单可选 100 股，清算遇空卖档 900 零成交。
   修复+补 Abort 后拍卖成交反例测试。
2. **civil 失败全量回滚**：drive.rs:162 在 civil 调用前已日结 World、消耗清簿事实、
   推进 driver.next_seq、填 drained_events——失败只恢复 session 两字段。需撤销
   World 日结（Abort）、恢复 driver seq/事件缓冲、验证重试恰一次。
3. **前置校验真正共用**：实测 `injected_failure_runs_before_missing_npc_strategy_validation`
   默认路径返回注入错误、runtime 路径返回缺策略错误（不同判）。failure.rs:79 仍先存
   Result 再无条件安装；安装失败不撤销已推进的 tick/seq/账户；install_candles 底层
   收集仍可吞发送错误提前返回部分数据。
4. **无效断言真正全换**：batch3.rs:59 仍留 `|| true`/拒收只计数/无符号>=0/
   accepted>=canceled；batch4.rs:32 新增 `qty > 0 || qty == 0` 恒真且未检查冻结量清零。
   另补真实 T+1、逐订单终结闭环、拍卖实际量、turnover 回滚测试。
5. **next_due 入回滚**：group.rs:604 决策重排 next_due 但该字段不进 undo/hash——
   失败 tick→Abort→重跑同 tick 时 NPC 已排到未来，会少一次决策。session 写回只更
   candidate 不同步 attention_scheduler heap；构造器仍合成固定 seed/初始 due，
   不恢复 attention RNG 状态（恢复前后行为不同）。
6. **配置贯通 World 构造**：bridge.rs:79 仍硬编码 `history_len: 16`，设 32 时
   desk 最多给 16 条、恢复校验要求精确长度即失败。另修正"双撤"旧注释与
   失败盘点归因（抽查 case 先被非首 tick 门禁拒绝，未触达声称缺失域）。

## 性能与验证口径勘正

- 探针复跑 689.5/667.5 t/s、守恒通过、无删事件域提速证据——数字不虚。
- 但探针直跑 World，**不含 driver 的 attention/行情/K 安装，且 auction_ticks=0**——
  与 318.0 t/s 那次（全语义含事件域上行）**不同口径**，"+97% 全由空档防御产生"
  无配对证据，不得混用两个口径报数。
- runtime 96 项复跑通过；`step_frame_equivalent_advances_tick_and_poisons_on_failure`
  单独复跑触发 9s deadline，97/97 未确认；lib 1548/0/11 未全量复跑确认。
- 失败清单"消 7/增 7/总 106"数字属实，但新增项逐域归因不成立（见修复集第 6 条）。
