# 在途卖单风险观察口径修复（2026-11-02）

分支：`codex/pending-sell-risk-20261102`（基于 `main` 0f40ffbc 的独立 worktree）。

## 缺陷

`runtime::group::group_urgency_at_view` 的权益读面
`equity_cents_with_pending` 按“在手 + 在途卖单实时余量”估值，但传给
`build_account_risk_observation` 的 `RiskPositionInput::qty` 取
`acct.shares`（挂出时已实扣），且 `shares == 0` 的纯在途股整条被跳过。
同一份观察因此“权益含在途、持仓不含在途”：回撤被放大、仓位权重被压小、
`reducing` 被误判为无可减仓位。

## 修复

- 新增 `for_each_held_position` 单次扫描：同时累计含在途权益并按每股
  qty>0 回调持仓（权益/风险持仓/复审资源共用账户级总持仓，每股 O(1)）。
- `account_risk_observation_at_view`：风险持仓 qty 改为含在途实时余量；
  纯在途股入观察；`qty==0` 才跳过。
- `reducing` 改用同一含在途持仓量。
- 缺行情/估值溢出仍由权益读面显式 panic，不引入默认价。

## 官方依据与口径

- 项目现行规则登记：卖出申报不得超过“持仓 − T+1 锁定 − 已挂卖单”，
  依据为沪深交易规则现行版本（`docs/trading-rules.md`，最近内容核对日期
  2026-09-22）。本轮未重新核对条文正文，沿用已登记结论。
- ADR-0043：runtime 用所有权转移模型，卖单挂出即实扣股份（实体在途）；
  总量语义与串行兼容管线的“持仓未扣 + reserved_sell_qty”等价。
- 未启用 runtime 的串行兼容管线（WASM）`plan_execution_urgency_at_view`
  直接读 `Account::positions()`（未实扣），天然含在途卖单；Web 原生默认
  走 World/RuntimeDriver 多线程。

## 证据

- `evidence/red.txt`：把生产实现临时回退到旧口径后的运行结果，
  4 passed / 3 failed（权益 100000≠300000、多股 300000≠500000、
  offset 用例缺股 panic）。
- `evidence/green.txt`、`evidence/green-final.txt`：修复后 9/9 通过，
  单命令 0.02~0.10s（`run-with-deadline.mjs 10000`）。
- 性能探针（临时，未提交）：5 股/1 在途票、debug 构建，完整风险观察
  ~6.3µs/call，相对旧口径 ~5.6µs/call 开销 12.6%（<20%）；最终实现每股 O(1) 读
  账户级镜像，不再查 `ChildTicket`。探针已删除。

## 边界

- 未实现串行兼容管线与 runtime 的自动化双路径同输入对照：串行管线不存在
  “实扣在途”状态，只能以同一语义输入（持仓含在途）分别调用生产函数；
  现有 `institution_existing_reduction_uses_its_own_observed_drawdown`
  覆盖串行路径的回撤紧迫度。此项作为残余验证缺口如实登记。

## 独立复核处理（2026-11-03）

复核发现及处置：
- 已修：重复 `#[test]`（会导致 `-D warnings` 失败）→ 删除，测试恢复 7 项。
- 已修：`held_qty_with_pending_at` 的 `saturating_add` 静默截断 → 改
  `checked_add` + 显式 panic。
- 已修：测试只改票余量、未同步账户级 `pending_sell_qty` 镜像 → 测试夹具
  同时维护两者。
- 已按复核意见改为 checked 的 `shares + pending_sell_qty`：账户级镜像覆盖
  全部卖单（含外部/无票单），权益与风险持仓共用；新增无票全未成交/部分
  成交红测（`red-mirror.txt`：旧票口径 2 passed / 7 failed）及成本价边界
  （净成本为正、回收超投入不可用）。
- 未修（残余缺口）：串行兼容管线与 runtime 的同输入自动化对照未建立。
