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
  qty>0 回调持仓（权益读面与风险持仓面共用一遍扫描，每股只查一次在途票）。
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
- `evidence/green.txt`、`evidence/green-final.txt`：修复后 7/7 通过，
  单命令 0.02~0.10s（`run-with-deadline.mjs 10000`）。
- 性能探针（临时，未提交）：5 股/1 在途票、debug 构建，完整风险观察
  ~6.3µs/call，相对旧口径 ~5.6µs/call 开销 12.6%（<20%）；单次持股扫描
  使每股在途查找从两次降为一次。探针已删除。

## 边界

- 未实现串行兼容管线与 runtime 的自动化双路径同输入对照：串行管线不存在
  “实扣在途”状态，只能以同一语义输入（持仓含在途）分别调用生产函数；
  现有 `institution_existing_reduction_uses_its_own_observed_drawdown`
  覆盖串行路径的回撤紧迫度。此项作为残余验证缺口如实登记。
- 相邻发现（未在本次修复）：`run_chain_decision` 组装
  `PlanLifecycleReview.held_by_code` 仍用 `acct.shares`，与串行路径的
  `positions()` 口径存在同类差异，需另立任务评估。

## 独立复核处理（2026-11-03）

复核发现及处置：
- 已修：重复 `#[test]`（会导致 `-D warnings` 失败）→ 删除，测试恢复 7 项。
- 已修：`held_qty_with_pending_at` 的 `saturating_add` 静默截断 → 改
  `checked_add` + 显式 panic。
- 已修：测试只改票余量、未同步账户级 `pending_sell_qty` 镜像 → 测试夹具
  同时维护两者。
- 未修（登记为后续任务）：`held_qty_with_pending_at` 仍只从计划链
  `ChildTicket` 加回，而账户级权威镜像 `pending_sell_qty` 覆盖**全部**卖单；
  若出现无 `ChildTicket` 的在途卖单，风险观察仍会漏记。
  `RuntimeDriver::inject_player_intents` 可路由任意 owner，外部/无票卖单
  可能使两口径分离；仅正常计划链路径下两者重合。完整账户契约需改用
  checked 的 `shares + pending_sell_qty` 并补无票用例。
- 未修（测试覆盖缺口）：offset 用例直接调用 `group_urgency_at_view`，
  未经过 `run_chain_decision` / `GroupReviewHost` 生产调用点；两处源码目前
  都传 `self.roster_offset`，属防回归缺口而非现存错误。
- 未修（残余缺口）：串行兼容管线与 runtime 的同输入自动化对照、含在途
  持仓的 `cost_price`/`unrealized_return` 断言未建立。
