# C1a 纯日历模型实施候选

- 状态：实现和代表性短测已交付，待未实施者独立完整 diff 复核；不表示 mixed calendar 生产可运行。
- 独占目录：`.worktree/mixed-calendar-c1a`；分支 `feat/engine-mixed-calendar-c1a`。
- 基线：实际读取 `refs/heads/main` 为 `791858d468113d9ac8cdf9794a237b171cd41673` 后建立；最初把主工作区 HEAD `af0fdeb6` 误称 main，已在开写前更正，未使用其作为实施基线。
- 冻结设计 SHA-256：`554e69add726e7f30f06fee50c487071097d712d1149becc21cb21a2ad8b6a64`。根目录 `mixed-c1-design-review-2026-10-25.md` 的 `sol61_tree_review` 独立设计批准已核对；不把设计批准当作实现批准。
- 本地提交：`fc19fcb2` 为可编译 stub 与 20 类断言；`4556ecfe` 为纯函数实现。没有 main 合并、远端操作或新依赖。

## 范围与语义

生产 diff 仅新增 `packages/engine/src/session/exchange_calendar_model.rs`（模块内短测）与 `session.rs` 一条 `pub(crate) mod`。没有新增生产 caller，不触碰 CivilClock、GameSession getter、K、cache、SaveSlot、persistence、runtime、NPC、账户、计划、宿主或生成类型。

`calendar_day_model` 只使用显式传入的 `TradingCalendar`：先验证 start/current 运行范围、日期关系与绑定身份，再按去重 exchange 一次扫描半开区间 `[start,current)`，分别计算共享 union 和逐所会话数量；最后投影到原始 StockId。稀疏 ID 保留，同所不同股合法，重复 StockId 显式拒绝，闭市原因原样保留。

`calendar_clock_projection` 验证 layout、显式 progress 与开闭市的一致性及 tick/day 精确关系；所有边界溢出显式返回 typed 错误。分钟映射复用真实 `completed_market_minute_count`，输出区分会话数量、当日 ordinal、已完成分钟数量与绝对分钟键。ClosingAuction 可已有 240 分钟但会话未完成；TradingCompleted 不重复增加分钟；闭市没有当前日首分钟键，但可有上一真实会话的末分钟键。

政策依据为已冻结游戏日历与本所休市规则。2026-10-26 重读并核对根目录已归档官方原件：SSE 2026 规则 2.4.1–2.4.2（docx SHA `fc922c433438b2636cb631eab25cca405209712acbb6aaded768c45456ff8888`）、SZSE 2026 规则 2.3.1–2.3.2（pdf SHA `9b66f8b0db70f84a25ef1ccb4ee2351001724e408117552d75f6d8993483c586`），两所分别按本所公告休市；生效日为 2026-07-06。未下载新版本替换原件，未新增真实年度 Official 数据。2030 SSE 覆盖仅为 `synthetic-test`；240 分钟属于既有游戏分钟口径，未冒称现实连续竞价恰有 240 分钟。

## TDD 与实际执行

先编译可执行 stub，再跑 A01–A20：20/20 因模型断言失败，不是缺失 API 的统一编译失败。A01 闭市股被错误标记 ordinal `Some(0)`；A04/A20 半开数量错误；A03 末 tick 未完成会话；A13/A14 非法进度被错误接受。日志 `red-a*.log` 保存 libtest 实际 exit 101 与断言详情；driver 把非零归为 exitCode 1，但不覆盖原始错误消息。

实现后同一 20 类断言全部通过，日志 `green-a*.log` 与 `green-results.json` 保留结果。每 case 为独立进程树 10000ms deadline，8 个 worker 并发；整批另由仓库 supervisor 提供 10000ms 进程树 deadline。最终一批最长 case 见原始日志，全部远小于 10s。

UnknownStock 查询在初始 stub 中已正确实现，故 A09 的初红命中闭市 ordinal，不能冒称 UnknownStock 的前置 TDD 红。另做明确的绿后负对照：R1 把所有查询错改成首股，先红在 SZSE exchange 被返回 SSE；R2 仅对未知 StockId fallback 首股，实际红在 UnknownStock 拒绝断言（`unknown-negative-r2-a09_*.log`）。两者是额外断言敏感性检查，不回填成前置 TDD。生产源码未应用变异。

## 源码、构建与证据边界

真实 engine `cargo build -p engine --lib --locked -j 8` 在红阶段完成，70180ms。独立 target 位于仓库根 `.tmp/mixed-calendar-c1a/target`；jobs=8、codegen-units=16、debug=0、incremental=false，共享进程树 deadline 300000ms；采样 rustc 147% CPU、7 threads。未复制或链接其他 target artifact。

随后红/绿都使用 `rustc --test harness.rs`：harness 只从该实际 engine rlib re-export `calendar/observation/stock`，并按路径直接编译当前待审模块。不是替身 Calendar、分钟映射或 StockId。最终 green 模块确已实际编译并执行，编译耗时低于 1s；真实 engine rlib 本体仍来自红阶段 stub，未声称最终完整 engine lib 已再次构建。遵主任务资源窗口约束，没有第二次大 lib 编译、全 engine 单测链接或全量回归；后续整合需要补最终树实际 lib 绑定。

完整 cold build 保留基线约 500 条 warning，新增模块未接生产 caller 的 dead_code 位置 14 处；没有增加 allow 抑制。最终独立 green harness 编译无 warning。`rustfmt --check` 与 `git diff --check` 通过。

`source-artifact-manifest.json` 保存最终源、red/green/negative 实际二进制与真实依赖 rlib 的 SHA-256。`red-module.rs` 保存初始 stub 原件；两个 `unknown-negative*.patch` 与生成脚本保存负对照变异。`run-validation.mjs` 使用仓库现有进程树 supervisor；`harness.rs` 为只包含待审模块及真实依赖的编译入口。

复现命令（普通 cases 命令每次均受外部 10s deadline）：

```sh
node agents/branch-completion/mixed-calendar-c1a/run-validation.mjs harness green
node scripts/run-with-deadline.mjs 10000 -- node agents/branch-completion/mixed-calendar-c1a/run-validation.mjs cases green
python3 agents/branch-completion/mixed-calendar-c1a/prepare-negative-controls.py
node agents/branch-completion/mixed-calendar-c1a/run-validation.mjs harness unknown-negative-r2 unknown-negative-r2-harness.rs
node scripts/run-with-deadline.mjs 10000 -- node agents/branch-completion/mixed-calendar-c1a/run-validation.mjs cases unknown-negative-r2 a09_
```

最后一条预期失败。重建 engine lib 属另行排窗口的长编译，不应和 runtime 冷编译并发抢 I/O。C1b/C2/C4/C5、实际 restore 与 mixed 可玩验收全部仍在本片范围之外。
