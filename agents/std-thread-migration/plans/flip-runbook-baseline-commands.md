# 318.0 / 627-689 基线命令重建（flip-runbook 风险②收口）

> 2026-10-17 由只读重建 subagent 起草。方法：翻转批提交链 commit message
> （e841ba4f…62ff4c9b…5412fd10）与 .tmp 台账交叉核对。**未找到任何当时
> 的原始 stdout 存档**（.tmp 全目录无 "结果：…t/s" 探针输出捕获）——本文件
> 是命令级重建，非输出级复现；终测仍须按 runbook §3.1 记录 binary SHA。

---

## 0. 结论一页

1. **318.0 与 627-689 同源同命令**：都出自
   `cargo run -p engine --release --example runtime_s5_probe -- 3000 10 2000`。
   318.0 = 62ff4c9b（2026-10-15 09:42，P1 空档防御**之前**）；
   627.3 = 5412fd10（10:58，P1 **之后**），Sol 复跑 667.5 / 689.5。
2. runbook §3.3 的 "口径 B = max_speed_probe + default-setup.json"
   **不是产生 318.0 的命令**——那是 2026-10-16 盘点 agent 按 Sol recheck
   措辞重构的推测命令，翻转批期间从未运行或归档过（证据链 §2）。
3. 真正经 GameSession→RuntimeDriver 的全语义口径（driver
   attention/行情/K 安装+事件上行）**至今没有任何已归档测量**；且当前
   driver 路径硬编码单组（drive.rs:154 `spec.n_groups = 1`、:272
   `accounts_per_group = npcs`），**无法产生 "10组×2000" 负载**。

---

## 1. 证据链（逐条注出处）

| # | 证据 | 出处 |
|---|------|------|
| 1 | "性能终测 318.0 t/s（**守恒全绿**；口径含生命周期/竞价/双腿事实上行）"——"守恒全绿" 全仓库只有 runtime_s5_probe 会打印（examples/runtime_s5_probe.rs:106-118 两条 assert_eq + "守恒：…✓"） | 62ff4c9b commit message |
| 2 | "3000 tick/10组×2000 NPC/5 股" 与探针位置参数逐一对应（args 3000 10 2000；n_stocks 硬编码 5，runtime_s5_probe.rs:22） | .tmp/sol-flip-review-prompt.txt:88 + 探针源码 |
| 3 | 当轮实现 agent 亲笔："627.3 t/s 口径待核…重跑命令：`cargo run -p engine --release --example runtime_s5_probe -- 3000 10 2000`" | .tmp/handoff-remediation2.md:42-44（2026-10-15 11:19） |
| 4 | "性能 627.3 t/s（**+97%**…空档防御）"——318.0×1.97=626.5≈627.3，实现者按**同口径**自比（若 318.0 是异口径，此比从源头即不成立） | 5412fd10 commit message |
| 5 | 同族数字链全部同探针（全部伴"守恒全绿"字样）：批8a 199.8-207.5（e841ba4f，探针创建提交）→ 批8b 381.0→425.0（fdb26b16，msg 明写"s5 全语义口径"）→ 批8收官 "事件域口径 332-384"+355.7（f3ba47e8，明写"上一提交探针"）→ 批2 124.4（76c26e22）→ 批2' 346.6（856b6c3f）→ 318.0（62ff4c9b）→ 627.3（5412fd10）→ 689.5/667.5（sol-recheck-verdict 复跑） | 各 commit message |
| 6 | 探针文件翻转批期间零改动：`git log -- packages/engine/examples/runtime_s5_probe.rs` 仅 e841ba4f 一条；62ff4c9b→HEAD diff 为空 | git |
| 7 | 346.6→318.0 的漂移有 src 变更可解释：其间 6e2d66b7（批3 日界编排闭合）改了 desk/drive/protocol/world/failure 共 53 行 src | git show 6e2d66b7 --stat |
| 8 | driver 路径无法 10×2000：bridge.rs:72 缺省 `n_groups: 1`（"组数按名册规模推导…接线批补"——至今未补）；drive.rs:154 `spec.n_groups = 1`、:272 `spec.accounts_per_group = npcs.max(1)` | 源码 |

**矛盾登记（供 Sol 终验收裁决）**：sol-recheck-verdict.md:38 称 318.0 为
"全语义含事件域上行"口径、与探针"不同口径"。该表述的依据是
sol-flip-review-prompt.txt:88 的"runtime 路径正式口径…**全语义**、守恒
全绿"——而"全语义"正是探针 doc 注释自称（runtime_s5_probe.rs:4-5
"撮合/费用/相位/行情/T+1/日结全语义"），"runtime 路径"指 World runtime
本体（探针即直跑 World）。Sol 的口径二分与提交链证据（#1-#5）冲突，
两者必有一错；本文件按提交链+实现者亲笔交接（#3）重建。终测报告引用
318.0 时须携带本条声明。

---

## 2. 口径 A：裸引擎探针（直跑 World）——精确命令（含 318.0 与 627-689）

```
cargo run -p engine --release --example runtime_s5_probe -- 3000 10 2000
```

binary 构建与绑定（runbook §3.1 verify-build 纪律）：

```
cargo build --release --example runtime_s5_probe
sha256sum target/release/examples/runtime_s5_probe
```

参数解释（源码 runtime_s5_probe.rs:18-73，自 e841ba4f 起未变）：
- arg1=3000：ticks；arg2=10：n_groups；arg3=2000：accounts_per_group
  （10×2000=20000 NPC，retail 决策内联）；n_stocks=5 硬编码（沪/深交替，
  初始价 1000+i×25，limit_bps=1000）
- 负载固件：initial_cash=1_000_000_000/人；初始股 1000 股×5 股/人；
  `StrategyData::retail(0.30, 200, 0.35)`；attention_gap_ticks=4；
  ticks_per_day=240（3000 tick≈12.5 交易日，含日结 Drain 清簿）；
  **auction_ticks=0（无开盘竞价）**；closing_auction_ticks=3（收盘竞价）；
  order_ttl_ticks=1_000_000（委托事实上无到期）；chunk=512；
  watchdog 30s/120s；rounds_cap=4
- 判据：末行必须打印"守恒：现金+费用=初始 ✓；股份=初始 ✓"（两条
  assert_eq）；任一守恒红=性能数字作废并升级正确性事故
- 单轮时长量级：3000/650≈4.6s（627-689 档）～3000/318≈9.4s

**预期数量级**：对照 **627-689 t/s**（627.3 实现者/667.5+689.5 Sol 复跑，
均采于 5412fd10）。HEAD 在其上叠加 D0-D5（组线程观察基/信念投影/相位
计时等进入 World·group 路径）与 Sol 整改批——量级未知，以本轮实测为准；
显著低于 627 按负结果先归因流程处理。

**可比性声明**：
- 与 627-689：同命令、同负载、同探针；代码版本不同（5412fd10 vs HEAD）。
- 与 318.0：同命令、同负载；但 318.0 采于 P1 空档防御落地**之前**
  （62ff4c9b），不是 HEAD 的同代码状态对照。若需复现 318.0 量级仅限
  归因用途：`git checkout 62ff4c9b` 后跑同命令。
- 原型 own-inline 962 t/s 为简化口径（无 FillFact/回执/撤销日志上行），
  只作背景，禁止与本口径并列换算（runbook §3.3 既定声明沿用）。

---

## 3. 口径 B：driver 全语义（经 GameSession.step 的 runtime 路径）——命令从未存档，318.0 不是它产生的

runbook §3.3 现有命令（盘点 agent 重构，**非历史命令**）：

```
ENGINE_STEP_RUNTIME=1 cargo run -p engine --release --example max_speed_probe -- \
  agents/engine-max-performance/default-setup.json 3000 <物理核数>
```

核实结果：
- 载体存在：examples/max_speed_probe.rs（45 行，翻转批期间零改动，最后
  修改于 33027fcd）。`GameSession::new_with_compute` + `step_with_phase_timing`
  循环，输出 JSON 的 `speed_x` 字段即 t/s；threads 参数只喂
  `ComputeContext::with_workers`（runtime 路径不消费它）。
- 语义成立：ENGINE_STEP_RUNTIME=1 时 step 分流 `step_via_runtime`——
  failure.rs:482-492 `install_market_views`+`install_candles`、drive.rs:1593
  `set_runtime_attention`，即"driver attention/行情/K 安装+事件上行"。
- 但**负载不是 10组×2000**：default-setup.json = 5 股 / 20000 retail +
  5 inst + 2 hot = 20007 NPC；且 driver 构造硬编码 **1 组×20007 账户**
  （drive.rs:154/:272）。也无守恒断言（守恒只在 s5 探针）。
- 因此该口径**没有任何历史数字**；318.0 归属口径 A（§1-§2）。终测若要
  此口径：用上面命令**新建基准**（记录 binary SHA+≥3 轮），预期数量级
  无对照、首轮实测建立；引用时明确"新建基准，无历史可比数字"。

### 3.1 若要求 "10组×2000 经 driver"（runbook §3.3 字面负载）——需新建

- 现无载体：组数推导"接线批补"至今未补（bridge.rs:72 注释），driver
  恒 1 组。**探针层无法修正组数**，须改 drive.rs 的 WorldSpec 组推导
  （探针外产品代码改动，超出测量归档范围，需单独立项）。
- 最小新建方案（不动组数、对齐 NPC 构成与守恒）：
  1. 复制 max_speed_probe.rs 为新 example（如 `driver_semantics_probe`）；
  2. 尾部加终态对账（session 读面汇总现金+费用/股份 vs 初始，打印 ✓，
     对齐 s5 探针判据形态）；
  3. setup 用 default-setup.json 的副本去掉 inst/hot（20000 纯 retail），
     NPC 构成对齐 s5 探针；组数仍为 1，报告中如实登记"1×20000，非
     10×2000"；
  4. 参数沿用 `<setup.json> <ticks> <threads>`。
  本次任务禁改 packages/，以上仅为方案，未落码。

---

## 4. 给 runbook 的勘误建议

- §3.2 对照数字 627-689：命令正确，无需改。
- §3.3 对照数字 318.0：**应移到 §3.2 作口径 A 的历史链**（P1 前数据点），
  或在其"对照数字"处改注"318.0 经查为口径 A 探针在 62ff4c9b（P1 前）
  的值，非本口径数字；本口径无历史对照，首轮实测建基准"。§3.3 的
  "低于 318.0 属预期候选"预警同样建立在误归属上，随此一并修正。
- §4.4 Sol 终验收第 4 项：请 Sol 对 §1 矛盾登记 Explicitly 裁决。
