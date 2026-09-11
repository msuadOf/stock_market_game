# HANDOFF — company-information-npc-intentions（换机交接）

> 生成于 2026-09-11，由 Atlas orchestrator 会话 `opencode:ses_f718381c0ffeoyrwjl5y2jWuO0` 固化。
> 本文件是新机器上恢复工作的第一入口。读完后按「新机器恢复步骤」操作。

---

## 1. 当前状态

| 项 | 值 |
|---|---|
| 计划 | `.omo/plans/company-information-npc-intentions.md`（46 项：42 实施 + F1–F4） |
| 进度 | **26/46 已完成**（剩余：任务 27–42 + F1–F4，20 项未勾选） |
| 分支 | `codex/feat/web-ui-polish`（remote: `origin` = github.com/msuadOf/stock_market_game） |
| HEAD（交接提交前） | ~~`d2080fb`~~ → **`14cc965`** = `d2080fb` + `9347dec`（交接固化）+ `14cc965`（任务27 WIP 快照，见 §1a） |
| Boulder | `.omo/boulder.json` active work `company-information-npc-intentions-257aa474`，todo:23/24/26 completed |
| 引擎测试基线 | `cargo test -p engine` = **944 过 / 0 败 / 4 忽略**（40 套件）；`cargo test --workspace` = 1004/0/5 |

### 本轮（257aa474 会话）完成
- **任务 23**（此前已实施提交 e0e1b7f/8c1c845/2378d2b，本轮补验+勾选；复核 APPROVE 记录在 issues.md/learnings.md）
- **任务 24** 母单/工作单统一计划执行 → `ccf0490`；独立复核 **APPROVE**（task-24-review.md）。实施来自被打断的会话 ses_f71a2695effenlSBF9R8Yg8uiC，收尾会话 ses_f71705112ffeXnCFfuXajP7nGk
- **任务 26** 决策链接通+彻底删除共同 V → `5574a33`（53 文件）+ 复核 REJECT 修复 `d2080fb`；同一 reviewer 复审 **APPROVE**（task-26-review.md，含 REJECT→fix 全程）。要点：decision_chain.rs / company_assembly.rs / Market 4 参构造 / ComputeBackend 单一 MarketView / engine-gpu evolve_v+shader 删除 / extraction_replay 重新钉锚（旧→新值在 issues.md）

### 复核门禁状态
任务 1–26 全部有独立复核回执（`task-N-review.md`）；历史 REJECT 债（10/12/13/14/25）均已修复+复审通过。无悬空复核欠账。

### §1a 任务 27 状态（重要——新机器第一动作必读）
任务 27（新存档契约）在旧机器**两次派发均中止**：第一次空转探索（6min 零产出），续发 EXECUTE NOW 后实际写了 25 个文件（+719/−79）再次中止。已按 WIP 快照提交为 **`14cc965`**：
- **已验证**：`cargo check -p engine` 绿（0 错误）。
- **未验证**：全量测试状态未知；`tests/save_contract/` 尚未创建（任务核心验收套件缺失）；extraction_replay 锚点可能处于半迁移红态。
- **改动面**：persistence.rs、session.rs、decision_chain.rs、disclosures.rs、company_operations.rs、civil_clock.rs、plan_execution{,/types}.rs、closing/mod.rs、public_view.rs、plans/mod.rs、baseline_fixture.rs、server actor.rs + 14 个测试/fixture 文件。
- **新机器处置建议**：派 fresh deep worker（不要复用任何旧会话），给它本 WIP 的 commit 范围（`9347dec..14cc965`），指令二选一由 worker 先评估再定：**续作**（理解现有半成品→补齐 save_contract 套件→跑全部门禁）或**判定不可续则回退该 WIP 重新实现**（`git revert 14cc965` 或按文件 checkout `9347dec` 后重来）。任务 27 的完整规格与过渡态清单在 issues.md 1004–1140 行 + 计划任务 27 条目；worker 提示词要求 EXECUTE NOW 起手（本机两次空转教训）。

## 2. 下一步（新机器的第一个动作）

**任务 27：扩展新格式完整存档并移除旧格式专用路径**（依赖 26 ✅；阻塞 28–32）。
交接情报已备好：本仓 `.omo/evidence/` 之外的探索报告在旧会话 `bg_cff2b80e`（task-27 save surface）与 `bg_3afc8f29`（task-29 host contract）——**这两个报告的正文未落盘**，新机器直接重新 explore 或让 worker 自查（下方 §5 的要点摘录可用）。

之后的批次依赖图（并行批次用 `{}`）：
```
27 → {28, 29} → {30, 31, 32, 33} → {34, 35} → {36, 37} → {38, 39} → {40, 41} → 42 → {F1, F2, F3, F4}
```
- 37 与 38/39 需资源隔离（性能测量互扰），勿同机并行。
- 40 独占发布产物目录；42 必须最后；F1–F4 全部 APPROVE 才算完成。
- 28 依赖 5/26/27；29 依赖 15/26/27；30–32 依赖 27+29；33 依赖 29；34 依赖 30+33；35 依赖 30–33；36 依赖 28+35；38 依赖 1+36；39 依赖 27+36；40 依赖 30–32+34+35+39；41 依赖 34+38+39。

## 3. 新机器恢复步骤

1. 克隆/拉取本仓（push 状态见 §6），`git checkout codex/feat/web-ui-polish`。
2. 环境要求：Node 24.18、pnpm 11.19、Rust 1.96.1（版本文件在仓库根）。`pnpm install`。
3. **必跑** `scripts\wasm-build.bat`（Windows）/ `./scripts/wasm-build.sh`（Linux）：`apps/web/wasm-pkg` 被 gitignore，不重建则 web 构建/E2E 全挂。
4. 验证基线：`cargo test -p engine`（期望 944/0/4）。
5. 在 OpenCode 中执行 `/start-work company-information-npc-intentions`（boulder 会指引续作 27）。用户偏好：**任务辨别后同批并行，加大并行力度**；每任务必须独立 subagent 复核（AGENTS.md 门禁），复核通过才勾选。

## 4. 关键契约/陷阱速查（全文见 `.omo/notepads/company-information-npc-intentions/`）

- **learnings.md**：按任务分节的模块布局与数学契约（26 个任务的积累，接手任何模块前先查对应节）。**issues.md**：全部已登记偏差/简化/债务。
- 任务 27 必须接的**过渡态**（26 留下，issues.md 最新节）：恢复连续性走「重放经营 + adopt_all_pending」过渡实现，须改为真持久化 belief/plan/information/disclosure 游标并删除重放代码；pending events 队列对终态计划的 drain 规则待定稿。
- **RNG 纪律**：所有 per-NPC 采样（profile 派生、信念假设、注意力）必须独立种子流；绝不能内联消耗 self.rng（extraction_replay 锚点会红）。
- **存档**：K7 新格式只此一套严格 schema，无迁移器；旧形状走通用校验错误。`SaveSlot.civil_clock` 必填先例。
- **环境坑**：pnpm 不在 agent shell PATH（记录 blocked，不静默）；rust-analyzer 30s 超时（用 cargo check 等效）；PS 5.1 管道毁 UTF-8 源码/证据（只用 edit/write 工具 + `cmd /c` 重定向）；rustfmt 必须 `--edition 2021 --style-edition 2021` 且只跑自己的文件；ts_rs 生成物（apps/web/src/types/generated/*.ts）跑测试会被改写——`git checkout --` 还原已跟踪的，未跟踪的留给任务 29 收编（当前积压 ~44 个文件 + 4 个已跟踪漂移已在本交接提交还原）。
- **已知 clippy 旧债**（非阻断，任务 42 前 pnpm lint 门禁需清）：`behavior/decision.rs:239` unnecessary_filter_map（任务 1 起）；tests/analysis_profiles 3× unusual_byte_groupings（任务 17）；tests/experience_feedback 2× too_many_arguments + 1× bool_assert_comparison（任务 20）。
- **证据目录约定**：`.omo/evidence/company-information-npc-intentions/`（task-N-happy/failure/review）。仓库根 `E/` 目录已在本交接提交中移除（git rm），勿再用。
- **before 基线**：`evidence/before/`（10 seed × 2 场景 + manifest）是任务 38 after 对比锚点，勿动。
- **engine_error_events**（matrix 55–98/seed）是观测项不是失败项（learnings 任务 1 节）。

## 5. 任务 27/29 探索要点摘录（来自本会话后台探索，正文在旧会话）

**任务 27（save contract）**：SaveSlot 定义+save/restore 在 session.rs；persistence.rs:510-518 的 fundamental_value>0 校验已随 26 删除；需枚举 K7 全部新状态进档（registry/books/calendar policy+digest/publications/npc information/beliefs/price memory/watchlists/plans/urgency+allocation policy/RNG 流）；旧格式专用路径删除清单：SessionSetup V 字段（已删）、snapshot fundamental_value（已删）、`#[serde(default)]` 对新字段的静默容忍（K7 禁止）、Rust 旧 fixture 形状；恢复原子性既有测试必须保绿。512MiB/公司256/长度溢出检查挂在 Rust restore 路径。

**任务 29（host contract + TS）**：engine 侧新增 company/query.rs 公共 DTO（页大小 20/最大 100/稳定报告 ID 游标，i128/u64 十进制字符串复用 AccountingAmount 先例）；PublicLibrary 查询面（information/queries.rs）是 DTO 投影源；宿主协议按 ADR0010 baseline/delta/seq；需新增 CivilDateAdvanced/CompanyDisclosurePublished 事件；`pnpm types:generate`/`check` 在 package.json；TS 侧 save-schema.ts:148-202 的 schema_version 特判与 defaults.ts V 字段迁移归 29；生成目录单 owner，收编全部积压绑定。

## 6. Git 交接状态

- 本交接提交包含：`.gitignore`（选择性忽略 `.omo/run-continuation|drafts|t14-*`，**不再 blanket 忽略 .omo/**）、`E/` 移除、`.omo/{plans,notepads,evidence,boulder.json,start-work,HANDOFF.md}` 全量固化。
- **未 push**。remote=origin(github)。push 属对外操作需用户确认——若以 push 交接，新机器直接 clone/fetch 分支 `codex/feat/web-ui-polish`；若不走远端，用 `git bundle create stock_market_game.bundle --all` 单文件拷贝。
- 用户未提交改动的保护约定：任何 reset/clean/stash 前先保存清单（计划 Execution strategy 节）。
