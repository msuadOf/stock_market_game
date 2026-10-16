# std-thread 迁移·默认翻转 runbook（翻转+终测+终验收执行手册）

> 2026-10-16 起草（只读盘点 agent）。供协调人按册执行；工作区=
> `/data1/baiyifan/workplace/stock_market_game/.worktree/std-thread-model`
> （branch `exp/std-thread-model`），所有命令在该目录执行。
> 依据：`.tmp/multi-tick-design.md` §5 风险清单、CLAUDE.md 多核测试纪律、
> 批 9 盘点 `.tmp/batch9-external-cleanup-inventory.md`、Sol 历次裁决。

---

## 1. 前置检查清单（全部通过才进 §2）

| # | 检查项 | 命令/证据 | 通过判据 |
|---|--------|-----------|----------|
| P1 | env=1 失败清零 | `ENGINE_STEP_RUNTIME=1 cargo test -p engine --lib -- --test-threads=8` | 0 failed（已知 flake 家族 npc_decisions/watchdog 单跑复验绿后登记，不算阻断） |
| P2 | 默认（旧管线）全量绿 | `cargo test -p engine --lib` | 0 failed / 11 ignored 基线形态 |
| P3 | 翻转批 1-5 全绿 | `cargo test -p engine --test runtime_flip_batch1 --test runtime_flip_batch2 --test runtime_flip_batch3 --test runtime_flip_batch4 --test runtime_flip_batch5` | 全绿（参考计数 10/13/14/14/15） |
| P4 | **批 9 先行项：wasm32 编译核实** | `rustup target list --installed`（无则装）→ `cargo check -p engine --target wasm32-unknown-unknown` | 编译通过；**若断链**：std::thread runtime 无法跑纯 wasm32——按批 9 盘点 §3 属产品级决策（RuntimeDriver wasm 串行后端 / wasm 面"暂不支持" / 其他），**上报用户拍板后才能翻转**（Web 优先是 Stage 1 目标，不得静默牺牲） |
| P5 | D 批收尾确认 | 台账（`.tmp/multi-tick-design.md` D3/D5 各节 + 最新交接文件）末态登记完整：全部失败有归因（已修/有意分歧已登记/D5 范围已闭合） | 无"待办/暂缓"类未登记残留；`step_pipeline_contract` 类旧管线专有注入面测试（D2 登记）确认为有意走旧管线 |
| P6 | 工作区干净 | `git status` | 仅 .tmp 等不入库产物；实现 agent 的 D5 收尾提交全部落库 |

**任一不过 → 停，回到实现 agent，不做条件翻转。**

---

## 2. 翻转步骤

### 2.1 翻转提交（单一目的提交，不混任何无关改动）

改动点唯一：`packages/engine/src/runtime/mod.rs` 的 `step_runtime_enabled()`
（现 :81-86）——`unwrap_or(false)` 改 `unwrap_or(true)`，并**同步改写**其上
doc 注释：缺省=runtime（std-thread 生产缺省）；`ENGINE_STEP_RUNTIME=0`
= 显式回旧管线的临时逃生门（批 9 外部清理时移除）。

提交规范（Conventional Commits + 尾行署名）：

```
feat(engine): step() 缺省翻转 std-thread runtime——生产缺省切换

- runtime/mod.rs step_runtime_enabled 缺省 false→true；显式
  ENGINE_STEP_RUNTIME=0 仍可回旧管线（批 9 清理前的逃生门）
- 前置：env=1 全量 0 失败、默认全量绿、翻转批 1-5 绿、wasm32 核实通过
- 翻转后立即全量回归（命令矩阵见 .tmp/flip-runbook.md §2.2）

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
```

### 2.2 翻转后立即全量回归命令矩阵（顺序执行，任一红即停）

| # | 口径 | 命令 | 判据 |
|---|------|------|------|
| R1 | 默认=runtime，engine lib | `cargo test -p engine --lib` | 0 failed（与 P1 同名单口径——翻转后默认即原 env=1 面） |
| R2 | engine 集成测试（71 个 test 目标，默认=runtime） | `cargo test -p engine --tests` | 0 failed（含 runtime_flip_batch1-5；--tests 不含 lib，R1/R2 都要跑） |
| R3 | 逃生门：显式回旧管线 | `ENGINE_STEP_RUNTIME=0 cargo test -p engine --lib` | 0 failed（证明逃生门与旧管线未在翻转中被破坏） |
| R4 | doctests | `cargo test -p engine --doc` | 0 failed |
| R5 | 全 workspace 长验收（server/desktop/web/scripts，含进程树 deadline 治理） | `node scripts/run-full-regression.mjs` | 全绿（server 侧 ComputeContext/rayon 引用在批 9 前仍在，属预期共存，不因回归红而顺手删——那是批 9 的活） |

说明：
- `step_runtime_enabled` 用 `OnceLock` 首读缓存——同进程内不可中途切换，
  R3 必须独立进程跑（上面命令天然满足）。
- R5 属长验收：共享 deadline、多核并行由脚本治理；跑前确认无其他完整
  验收批次在途。
- 回归全绿 → 打 tag 或记录翻转 commit SHA（§3/§4/§5 都引用它），随后才可
  进入性能终测与 Sol 终验收。**在此之前批 9 外部清理不得开工**（它以翻转
  落地为前提，且会删逃生门相关暴露面）。

---

## 3. 性能终测协议（两口径分开报，禁止跨口径相减/换算）

### 3.1 独占基准窗口（硬性）

- 正式基准期间**其他 agent 一律暂停**（编译、测试、探针全停）——核段隔离
  不能防测量污染（用户既定规则，见 memory 基准独占全机条目）。
- 每轮跑前记录：日期时间、各核占用（/proc/stat 快照）、binary SHA
  （`sha256sum target/release/examples/<probe>`——verify-build 纪律：
  探针结论必须绑定 binary，正确性结论同样适用）。
- 多轮取样：**每个口径 ≥3 轮**，同口径背靠背跑完再换口径；报全部轮次 +
  中位数 + 极差；轮间漂移 >5% 加跑到 5 轮并在报告中如实标注。

### 3.2 口径 A：裸引擎探针（直跑 World）

```
cargo run -p engine --release --example runtime_s5_probe -- 3000 10 2000
```

- 负载：3000 tick / 10 组×2000 NPC（20000，retail 内联）/ 5 股；
  `auction_ticks=0`、无 driver 的 attention/行情/K 安装（探针 WorldSpec
  如此构造——本口径定义即"裸引擎"）。
- 判据：守恒两断言必须打印 ✓（现金+费用=初始；股份=初始）；任一守恒红
  =性能数字作废并升级为正确性事故。
- 对照数字：**627-689 t/s**（627.3 待核 → Sol 复跑 667.5 / 689.5 t/s，
  守恒通过）。
- 注意：D0-D5 给 runtime 增加了常驻 driver/决策链/投影面，本探针直跑
  World 不经 driver，理论受影响小，但以本轮实测为准。

### 3.3 口径 B：全语义含事件域上行（经 GameSession.step 的 runtime 路径）

```
ENGINE_STEP_RUNTIME=1 cargo run -p engine --release --example max_speed_probe -- \
  agents/engine-max-performance/default-setup.json 3000 <物理核数>
```

（翻转后可省 env——缺省已是 runtime；保留 env 使命令在翻转前后同义。
threads 参数沿用旧正式口径形态；runtime 路径不消费 ComputeContext。）

- 负载：default-setup.json = 5 股 / 20000 retail + 5 inst + 2 hot（20007
  NPC，≈10 组×2000 口径）；经 driver：attention/行情/K 安装 + 事件域
  上行 + 生命周期/竞价/双腿事实，3000 tick。
- 对照数字：**318.0 t/s**（翻转批 HEAD 62ff4c9b 时的正式口径）。
- **口径 B 特别注意**：318.0 采于 D0-D5 之前；D0-D5 补齐的是语义完整性
  （决策链/信念/复审/存档域——每 tick 更多上行与投影工作），终测数字
  **低于 318.0 属预期候选**，不等于回归。若显著低于 318.0，按"负结果先
  归因"流程（Astra 独立审查：语义完整性代价 vs 实现退化 vs 测量问题）
  再下结论，不许直接报"性能达标/不达标"了事。
- 另附不可比参照：原型 own-inline **962 t/s**（简化口径，无 FillFact/
  回执/撤销日志上行）——只作背景，禁止与本两口径并列比较。

### 3.4 报告格式（写进 .tmp/性能终测结果，供 §4 Sol 交叉核验）

每口径一节：命令原文 / binary SHA / 核占用协变量 / 各轮 t/s / 中位数 /
守恒或事件计数证据；末尾明确写"两口径分开陈述，未做跨口径换算"。

---

## 4. Sol 终验收范围（独立 subagent，未参与实现）

Sol 输入材料与必答项：

1. **迁移全链 diff 摘要**：`git diff a6001494..HEAD --stat`（merge-base
   a600149469a7d14be5b3dc682119ce07012af991，现 219 commits）+ 分域导览：
   `packages/engine/src/runtime/`（新模块：desk/group/world/coordinator/
   drive/decision/plans/bridge/event_bridge/fees/watchdog）、session 侧
   接线（failure.rs step 分流、runtime_driver 挂载、投影回写群）、
   翻转批台账 `packages/engine/tests/runtime_flip_batch{1..5}.rs` +
   `tests/common/flip_helpers.rs`、`.tmp/multi-tick-design.md` D0-D5 记录。
   Sol 回答标准三问：大 A 语义是否符合且依据可靠 / 改动是否需求必需且
   最小 / 有无遗漏边界测试、跨层语义漂移、不必要复杂度。
2. **trading-rules 登记核验**：`docs/trading-rules.md` 分支内全部新增/
   修改条目——至少：自成交限制（含游戏简化声明：监控口径实现为撮合硬
   排除）、日终未成交单失效显式 OrderCanceled（有意分歧裁决）。核对每条
   的官方依据援引与适用日期是否可靠、简化/不支持标注是否如实。
3. **有意分歧清单裁定**：Sol 逐条验收已登记分歧，每条裁定"维持（理由）
   /需修复"。清单来源（汇总自 multi-tick-design.md 各批登记）：
   - civil 日历推进编排（runtime 自动 vs 旧管线宿主显式，ADR-0025 对齐）；
   - 拍卖"撤卖侧保买侧"上界语义（Sol 曾裁定维持）；
   - DeskTradeFact maker/taker 归一化（maker≡卖方/taker≡买方，desk.rs）;
   - frozen_cash=0（所有权转移口径 vs 旧管线 escrow 预留口径）；
   - 空簿不覆盖投影、exposed_stocks 起步、D3a-2c/D3a-3 各登记差异、
     在途卖单 qty 口径（部分已收口——Sol 核对收口提交与残留）；
   - `step_pipeline_contract` 类旧管线专有注入面测试显式走旧管线（D2
     登记；批 9 退役旧管线时随语义迁移）。
   分歧未在 trading-rules.md 或台账正式登记的一律视为漏项退回。
4. **性能口径核验**：§3.4 报告——两口径是否真分开、窗口是否独占、轮次/
   SHA/协变量是否齐全、对照数字引用是否诚实（尤其 318.0 的 D0-D5 前置
   语境与 962 不可比声明）。
5. **翻转本体**：§2.1 提交是否单一目的、逃生门语义（显式 0）是否成立。

Sol 有效发现→修复→再次复核；无法修复的明确报告。未过 Sol 不得宣称迁移
完成，也不得开始批 9。

---

## 5. 回滚预案

### 5.1 源级回滚（主路径）

```
git revert <翻转commit-SHA>     # §2.1 单一目的提交，revert 即净回旧管线缺省
```

回滚后验证命令（全绿才算回滚成立）：

| # | 命令 | 判据 |
|---|------|------|
| V1 | `cargo test -p engine --lib` | 0 failed（缺省已回旧管线=翻转前 P2 形态） |
| V2 | `cargo test -p engine --test runtime_flip_batch1 --test runtime_flip_batch2 --test runtime_flip_batch3 --test runtime_flip_batch4 --test runtime_flip_batch5` | 全绿（runtime 仍作为 opt-in 完整存在） |
| V3 | `ENGINE_STEP_RUNTIME=1 cargo test -p engine --lib` | 0 failed（证明 revert 只改缺省、未伤 runtime 本体——后续修复后可再次翻转） |

### 5.2 运行时逃生门（不改源码）

翻转后部署侧遇事故：环境变量 `ENGINE_STEP_RUNTIME=0` 即整进程回旧管线
（OnceLock 首读生效，须在进程启动前设置）。此门在批 9 清理开关本体时
移除——**移除前提=翻转后稳定期已过**。

### 5.3 回滚决策依据

- 正确性事故（守恒红/事件语义错）→ 5.1 立即回滚，事故单列排查，修复走
  新提交后再翻转；
- 性能不达预期 → **不构成回滚理由**：按 §3.3 归因流程拿数据再议
  （负结果先归因规则）。

---

## 6. 起草人附加风险登记（任务明示清单之外）

1. **wasm32 断链是翻转前最大单点**（P4）：批 9 盘点指出 RuntimeDriver/
   drive.rs 未按 wasm32 门控（`pub mod world` 门了，drive 没），翻转把
   runtime 变生产缺省的同时 Web 目标可能编不过——Stage 1 是"Web 优先"，
   必须核实+用户拍板，不得带病翻转。
2. **318.0 基线可复现性弱**：原始命令未单独归档（仅存 commit message 与
   口径描述），终测须以本轮记录的命令+SHA 建立新基准；如与本手册命令
   数字量级不符，先核口径（NPC 构成/拍卖/上行面）再归因。
3. **两口径负载不同构**：A=20000 纯 retail、auction_ticks=0、无 driver；
   B=20007（含 inst/hot）、含竞价与事件上行。分开报（任务要求）之外，
   报告中要显式声明二者不可互相折算。
4. **R5 与批 9 的边界**：全量回归里 server 侧 rayon/ComputeContext 共存
   是预期；若回归顺手"清理"它们=范围越界（批 9 专属，且有独立盘点）。
5. **翻转 commit 之后、Sol 验收之前**任何人不得往分支塞实现提交——
   Sol 审的 diff 必须冻结在翻转 commit；发现问题的修复走"新提交+重审"。
