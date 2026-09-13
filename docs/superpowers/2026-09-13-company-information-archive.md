# company-information 工作归档（2026-09-13）

> 本文件是 `.omo/`（编排工作区，即将整体删除）中 **company-information-npc-intentions**
> 工作流的归档入口：完成了什么、还有什么没完成、设计/策略/债务记录去哪找。
> 本目录其余文件为逐字副本；本文件为归档时新写的状态快照。
> 历史记录不是当前规则权威来源，见 [README.md](README.md)。

## 1. 工作总览

| 计划 | 规模 | 状态 | 归档副本 |
|---|---|---|---|
| company-information-npc-intentions | 46 项（42 实施 + F1–F4 终验） | **37 完成 / 9 进行中** | [`plans/2026-09-10-company-information-npc-intentions.md`](plans/2026-09-10-company-information-npc-intentions.md) |
| resolve-blockers-wayland | 14 项（10 实施 + F1–F4 终验） | **计划就绪，0 项执行** | [`plans/2026-09-13-resolve-blockers-wayland.md`](plans/2026-09-13-resolve-blockers-wayland.md) |

- 主计划交付：公司经营→复式记账→四行业（工商/银行/保险/地产）报表→公开披露→NPC 个体
  混合分析（基本面/趋势/量价/技术/经历）→跨日个人交易计划→真实订单执行→K7 新存档→
  三宿主（WASM/Server/Tauri）公共查询→公开财务 UI→dev 诊断隔离。
- wayland 计划是主计划剩余 9 项的**收口计划**：Weston Wayland 原生渲染/IPC 验证、
  可断点续跑的 K7 基线矩阵、三宿主 parity、release 隔离契约、文档同步、最终封存。
- Git 状态（归档时）：分支 `codex/feat/web-ui-polish`，HEAD `a36ef84`；任务 27 完成于
  `5ecb24c`，K7 基线检查点 `ace47be`，证据封存 `3c55963`。

## 2. 已完成（37 项，均含独立复核回执）

- **W1（1–7）**：多 seed before 基线；官方会计/日历依据登记；session/strategy 接缝抽取；
  真实公历+冻结交易日历；自然日经营时钟；原子复式记账底座；公司实体与开局账套。
- **W2（8–14）**：工商/银行/保险/地产四行业会计；固定集团合并与少数股东；五类报表与
  版本化结账；自然日经营演化与经济事件。
- **W3（15–21）**：定期报告/临时公告/不可变公开库；公共曝光与个人获知分离；身份/风格/
  分析权重解耦；个人基本面预测与三种估值方法；技术指标与价格记忆；经历接入信心与风险；
  跨日交易计划状态机。
- **W4（22–28）**：跨股软预算；观点/紧迫度分离；母单统一执行；关注发现与淡出；
  **决策链接通并彻底删除共同 V**（`5574a33`）；**K7 新存档契约**（任务 27）；端到端场景闭环。
- **W5（29–35）**：公共报告查询契约与 TS 类型生成；WASM/Server/Tauri 三宿主接通；
  前端公共公司状态；公开财务界面+起始日期选择（E2E）；dev 诊断与 release 隔离。
- **W6（36、39）**：离线因果诊断与量价指标（含午间时钟修复，复核 ACCEPT）；
  2万/5万/10万账户规模与长期存档成本实测。
- 证据：`.omo/evidence/company-information-npc-intentions/`（task-N-happy/failure/review 等
  227 个文件已被 git 跟踪，见 §5）。

## 3. 未完成（主计划 9 项 `[~]` + wayland 计划 14 项未开始）

| 主计划任务 | 已做到 | 缺口 | 接手（wayland 计划） |
|---|---|---|---|
| 37 三宿主 parity 矩阵 | 未启动（无 parity/ 证据） | 全部 | Todo 5 |
| 38 基线 after/敏感性 | 仅 primary 矩阵 seed-1 部分产物（336 MB，已 gitignore） | 10 seed 主矩阵、跨年 5×400 自然日、0.5/1/2 敏感性；且计划要求**新鲜可续跑**矩阵，旧产物不采信 | Todo 6（续跑器）+ 7（执行） |
| 40 release 契约验证 | 未启动（无 release/ 证据） | 全部 | Todo 8 |
| 41 文档全量同步 | 任务 2/36 已建 company-accounting / simulation-calendar / company-actions-design / causal-diagnostics；README 已记 Linux 依赖 | ADR0016 等受影响文档、清单矛盾修正、过时午间时钟表述（causal-diagnostics.md:27）、trading-rules 简化登记同步 | Todo 9 |
| 42 全套回归封存 | 未启动 | verify-plan.mjs + 干净 worktree 全门禁 | Todo 10 |
| F1–F4 终验 | 未启动 | 四项独立审查 | Wave 4 |

## 4. 已知未决债务（详见 [`specs/2026-09-13-company-information-issues.md`](specs/2026-09-13-company-information-issues.md) 与 problems 副本）

- **Task 31 复核发现（OPEN）**：民事日结算失败不得吞错、restore 须建新 public timeline/
  revision、resync 须发新 baseline+sequence gate、publisher 元数据跨 revision 段一致；
  另有 bearer 凭证入查询串、全量 Snapshot 序列化、嵌套分配维度独立门禁等观察项。
- **8 MiB server body 契约 vs 实测存档**：2万/5万/10万账户存档 29/69/133 MB 全部超限
  （上限未动，未静默抬高）；wayland Todo 4 负责给它成文的文档归宿。
- **Wayland 像素证据**：Weston 14 headless 输出零尺寸致 screenshooter 失败，原生启动/IPC
  有证据但无截图；wayland Todo 3 以 pixman/≥1280×800 重做。
- **C06 真实市场校准**：无授权数据源，明确未完成，不伪造。
- **clippy 旧债**：`behavior/decision.rs:239` 等（wayland Todo 2 清偿）。
- **TV 水印碎片**：桌面截图图表面残留，范围外，留给图表渲染工作。
- 简化登记（如无年终结账分录、报表窗口化推导）目前完整存在于 issues 副本；
  任务 41 完成前它是简化/不支持项的权威清单（AGENTS.md 大 A 语义门禁要求）。

## 5. 归档映射与数据留存

| `.omo/` 源 | 归档位置 | 说明 |
|---|---|---|
| `plans/company-information-npc-intentions.md` | `plans/2026-09-10-…md` | 46 任务 + K1–K7 固定契约（设计核心） |
| `plans/resolve-blockers-wayland.md` | `plans/2026-09-13-…md` | 收口计划，**含未提交的 +122 行扩写** |
| `notepads/…/learnings.md` | `specs/2026-09-13-…-learnings.md` | 逐任务模块布局、数学契约、宿主桥接陷阱 |
| `notepads/…/issues.md` | `specs/2026-09-13-…-issues.md` | 偏差/简化/债务登记簿（1476 行知识） |
| `notepads/…/problems.md` | `specs/2026-09-13-…-problems.md` | 阻塞与解除记录 |
| `HANDOFF.md` | `specs/2026-09-11-…-handoff.md` | 2026-09-11 换机交接快照（26/46 时点，环境陷阱速查仍有价值） |
| `notepads/…/decisions.md` | 不归档 | 空脚手架（仅标题） |
| `evidence/`（227 个已跟踪文件） | 不复制 | git 历史可找回：`git log --oneline -- .omo`、`git show <rev>:<path>` |
| `run-continuation/`、`drafts/`、`after/…/seed-1.json` 等 gitignore 项 | **删除后不可恢复** | 会话续跑状态、早期计划草稿（已被 plans 版本取代）、部分 QA 原始产物（按计划本就不采信） |

注意：

1. 归档副本取自**工作树**（含未提交修改：learnings.md +2 行、resolve-blockers-wayland.md
   +122/−19 行）。删除 `.omo/` 前若不提交这两个源文件，git 历史只留旧版；副本已是最新。
2. 笔记副本已于 2026-09-13 17:45 随并发会话追加而**重新同步**（learnings/issues/problems 及
   wayland 计划均为最新工作树内容）。归档期间检测到**另一并发会话**正在执行 wayland 计划
   （特征吻合 Todo 1/2/4/9：对账证据目录、corepack/clippy、文档契约脚本、docs 同步）。
   若该会话继续追加 notepad 条目，删除 `.omo/` 前应再次同步 specs/ 下三个笔记副本。
2. 主计划 QA 命令惯例以 `.omo/evidence/<plan>/` 为 `--output`；删除后执行 wayland 计划时
   需改用新的证据输出目录（脚本均接受 `--output`）。
3. 协作纪律教训（worker 禁止 reset/amend 共享分支、notepad 只准 append、证据要原始退出码
   不以空输出当成功）都在 issues 副本内，接手 worker 必读。

## 6. 后续工作

执行 [`plans/2026-09-13-resolve-blockers-wayland.md`](plans/2026-09-13-resolve-blockers-wayland.md)：
Wave 1（1–4）→ Wave 2（5–7）→ Wave 3（8–10）→ F1–F4。全部 APPROVE 后，
company-information 工作流才算整体交付（主计划 42 + 双 F1–F4 门禁）。
