# 交接文件：Sol 二审后整改（2026-10-15 晚，轮换）

## 当前状态

- HEAD：`5412fd10`（P1-1~P1-10 整改批，已提交，lib 1548/0/11 绿）
- 工作区：**5 文件未提交 WIP（D2 批半成品，编译不过——2 错误）**。
  接手人选择：继续修完（见下"坑"）或 `git checkout -- .` 丢弃重来。
  WIP 内容 = D2 玩家意图路由（ExternalPlan 通道+组端消费缓冲+
  drive 注入+step_via_runtime 接线），失败原因：
  1. failure.rs 里 `take_runtime_player_intents` 定义插错了位置
     （插到了 impl 外——E0401 can't use Self）；
  2. 由此连锁 E0599。
  WIP 设计本身可用：protocol.rs 的 `GroupIn::ExternalPlan`、
  group.rs 的 `external_pending` 缓冲+run_round 消费、drive.rs 的
  `route_player_intents/inject_player_intents`、world.rs 的
  `broadcast_external_plan`。修好插入位置即可编译。

## Sol 二审要点（.tmp/sol-recheck-verdict.md 有完整版）

- P1-9 汇报失实：batch3 恒真断言大量保留（我只改了 2 处 ||true，
  `剩余_qty>0||true` 改了但 **inventory 闭环断言的新版没检查
  traded+canceled>=accepted 在空事件集时恒过的边界**；batch4 我新增
  的 `pos.qty() > 0 || pos.qty() == 0` 是恒真——Sol 抓得对）。
  新 agent 必须逐条重审 batch3/4 全部断言，宁可删掉弱断言也不要恒真。

## D 批进度（对表完成、实现未开始有效部分）

- 新盘点：`.tmp/flip-failures-after-p1.txt`（P1 后仍 106：消 7 增 7）
- 批划分 v2：`.tmp/d-batch-plan-v2.md`（D2:20/D4:20/D3:24/D1:35/D5:8）
- 域分布：decision 29 / candidate 17 / binding 17 / t3t4 12 / save 7 /
  envelope 6 / plan 6 / other 6 / attention 3 / expiry 3
- 原盘点文档：`.tmp/flip-106-domain-inventory.md`（尾部有 P1 后重做节）

## 已知坑

1. **session/failure.rs 的 runtime 助手群**（P1 批引入 ~12 个 pub 方法）
   容易在 rustfmt 后丢失——那个文件我多次"checkout session 后重打"，
   新 agent 建议：把助手群移到独立文件（如 session/runtime_bridge.rs）
   一次性提交，别再玩 checkout-reapply 循环。
2. **stash 里有两个非我的旧条目**（arch-fix-23/engine-max-performance）
   别动；我的 d2-wip stash 已 apply 并处理（上面的未提交 WIP）。
3. **627.3 t/s 口径待核**：Sol 复审在验（空档防御带来的 +97% 是否
   真实收益 vs 减少了上行负载）。重跑命令：
   `cargo run -p engine --release --example runtime_s5_probe -- 3000 10 2000`
4. **best_bid/best_ask 空档防御**（P1 附带）改变了撮合路径——若 Sol
   裁定这是性能口径问题，回退方式=恢复 keys().next() 三行。
5. **ENGINE_STEP_RUNTIME=1 复现失败集**：
   `ENGINE_STEP_RUNTIME=1 cargo test -p engine --lib -- --test-threads=8`

## 关键文件索引

- runtime 核心：packages/engine/src/runtime/{desk,group,world,coordinator,drive,decision,plans,bridge,event_bridge}.rs
- 翻转入口：packages/engine/src/session/failure.rs（step/step_via_runtime）
- 迁移台账：packages/engine/tests/runtime_flip_batch{1..5}.rs + tests/common/flip_helpers.rs
- trading-rules 登记：docs/trading-rules.md（自成交/日终失效两条）
- Sol 材料：.tmp/sol-flip-review-prompt.txt / .tmp/sol-recheck-verdict.md
