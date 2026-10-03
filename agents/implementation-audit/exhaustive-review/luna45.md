# Luna45：company-information 归档 / Wayland 收口计划复核

审查基线：产品 `08e4fc7`（merge 同），工作树 `HEAD=a7c7ce3`。已先读仓库 `AGENTS.md` 与 `docs/principles.md`。目标文档完整连续读取至 EOF：归档 `docs/superpowers/2026-09-13-company-information-archive.md` 122 行、目录 `docs/superpowers/README.md` 11 行、正式计划 `docs/superpowers/plans/2026-09-13-resolve-blockers-wayland.md` 161 行，共 294 行；不是摘录代读。本轮只新增此审查记录，不改产品或 Git 状态，不运行测试/长验收。

## 逐章矩阵

| 文档位置 | 原文主张 / 当前 caller 与证据 | 复核结论 |
|---|---|---|
| Archive §1 (1–18) | 原文记主计划 37/46、wayland 0/14，并把 wayland 定义为收口；当前提交链已有后续实现与审计。`docs/work-status.md:319–322` 仍把 37/38/40/41/42/F1–F4 标作未完成，并将薄 `.omo` 模板与正式计划区分。 | 归档快照只能说明 2026-09-13 状态；不能当现况。正式计划仍明确未验收，薄模板不是权威执行副本。 |
| Archive §2、§2a (20–53) | 已完工作分类与复核回执清单；历史 task receipt 只能作为历史证据。Task36 clock 修复后续在 `docs/superpowers/specs/2026-09-13-company-information-problems.md:78–91` 记录 reviewer ACCEPT/receipt；`docs/implementation-gaps.md:174–176` 也记午间 clock 已修。 | 旧的 Task36 行为 blocker 已核销；历史回执缺口是否由正式 Todo1/终验补齐，仍看不到 `task-1` 收口证据，不得混同。 |
| Archive §3 (55–73) | 原文把 Todo5–10 作为后续。当前 `scripts/simulation/` 有 `baseline-run.mjs`、`audit-diagnostic-divergence.mjs` 与测试，但没有 `host-parity.mjs`、`release-contract.mjs`、`verify-plan.mjs`；work-status 仍记 37/38/40/41/42 未终验。 | Todo5/8/10 的指定验收工具与实际宿主证据仍缺；Todo6 的 runner 已存在，不能从旧归档“未启动”推断代码/CLI不存在；Todo7 仍须新鲜完整矩阵及 manifest。 |
| Archive §4 (75–88) | 原文称 Server 8 MiB 限额、Task31 多项待定、Weston 无像素证据、C06 无授权数据源、旧 clippy 债、简化清单留 issues 副本。现 `packages/engine/src/session/persistence.rs:944` 为 512 MiB，`apps/server/src/routes.rs:41` 为引擎额度 + 1 MiB；`docs/decisions/0019-draft-market-scope-and-capacity.md:29` 明确记录当前契约。`docs/implementation-gaps.md:174–176` 明确 8 MiB 与 lunch-clock 是过时报法。 | 8 MiB blocker 已被后续 ADR/代码 supersede，旧存档大小仍可说明实测值，但不能说当前请求体契约仍 8 MiB。C06 未校准不能推成交易功能故障；Wayland像素仍独立待证。 |
| Archive §5 (90–111) | 归档副本/源文件映射与 `.omo` 删除风险属历史保全说明，不是当前产品验收。当前 `.omo` 证据和计划目录仍在树内，可读取历史 task 文件。 | 可作为追溯来源；其“删除后不可恢复”等假设不适用于当前 checkout 的文件现状，也不授予恢复/清理行为。 |
| Archive §6 (113–122) | 原文安排 Wave 1→4，要求全部 APPROVE。当前正式计划仍含相同阶段，但 `docs/work-status.md` 未宣告全部完成，resolve-blockers 证据仅看到 `task-2-toolchain.txt`。 | 计划并未因归档而退役；全计划完成仍未证。 |
| README (全 11 行) | 原文声明 specs/plans 是历史记录、规则以 ADR/正式文档/engine 测试为准；最后特别声明 Wayland 计划“归档时尚未执行，属进行中工作”。`docs/work-status.md:322` 同样以正式副本作后续任务。 | 正确提醒读者不可把历史方案当规则；“进行中”是旧快照而非本日状态，须对照当前证据，不可据此声称还没任何实现。 |
| Plan TL;DR / Scope / Verification (1–34) | 承诺 Wayland 真实图像与 IPC、真实 parity、fresh K7、release/doc/final gates；反对伪图/grep/减矩阵；明确“8 MiB mismatch”。 | 范围本身清楚但 8 MiB 前提已被 ADR-0019 与现况改写；“具名记录历史测量/大存档容量余量”可保留，“当前 8 MiB mismatch”不可继续作为 Todo4 现况。 |
| Plan waves / dependency matrix (36–57) | Todo1–10 依赖顺序清晰；Todo3→5/8/10、6→7→9→10。 | 计划依赖关系仍然适用；其验收状态须逐 Todo 判，不从波次排布推出通过。 |
| Todo1 (61–67) | 要复核 31/32/35、补 Task36 acceptance receipt、clean pinned worktree 与 workspace test。已有 Task36 修复 review，但本轮未见 `.omo/evidence/resolve-blockers-wayland/task-1*`。 | 旧 clock 源缺陷已修，Task1 所需总收据/clean revision 仍未由找到的正式产物证明。 |
| Todo2 (69–75) | 计划要求 Corepack/web/clippy。`.omo/evidence/resolve-blockers-wayland/task-2-toolchain.txt` 实际记载 clippy exit 0、Node24/Corepack0.35/pnpm11 下 test/lint/types/tsc/build 通过，裸 pnpm 不在 PATH 时帮助脚本给出 .nvmrc 指引。`package.json:20` 和 `scripts/corepack-pnpm.sh` 是当前入口。 | Todo2 有比旧 Archive 更晚的实质修复/执行证据；不要再把早期 `Node25` 环境异常或 clippy 债作为现行失败。此证据不替代后续 Todo10 clean pinned replay。 |
| Todo3 (77–83) | 要真实 Wayland ≥1280×800 像素、Tauri app 区域及 IPC/清理。旧 Task35 文件有 Weston `wayland-info`、screenshooter 失败记录；README caller (`README.md:163–177`) 启 Weston 后运行 app、查询 wayland-info，但未构成像素断言、真实 invoke 交互或可靠子进程清理验收。计划 task-3 收口产物未见。 | GTK/WebKitGTK 安装后构建/MockRuntime/Xvfb 与真实 Wayland启动是不同证据；像素有效截图和真实 invoke dispatch 仍未证。计划将 actor tests “under Weston”表述需拆开：MockRuntime actor 单测不能被误报为 Weston 实际 IPC。 |
| Todo4 (85–91) | 原文指定将“8 MiB remote-body consequence”写进具名文档且不抬 limit。此接受条件依赖已废弃额度：当前额度 `512 MiB + 1 MiB`，ADR-0019 已批准，`docs/implementation-gaps.md:174` 将 8 MiB 句判为过时。 | 本 Todo 的 limit 判断必须 supersede；不得按旧计划恢复 8 MiB 或把“当前超限”写回。仍有价值的仅是历史测量档案、Wayland 边界、外部基准数据未获授权等经证实边界，并应按最新批准范围更新契约。 |
| Todo5 (93–99) | 需要 WASM worker、authenticated Server HTTP/WS、真实 Weston Tauri IPC parity。当前代码有三宿主各自的接口/调用方，desktop invoke commands 在 `apps/desktop/src-tauri/src/lib.rs`，server route 在 `apps/server/src/routes.rs`，WASM host 在 `apps/web/src/host/wasm-host.ts`；但无规定的 `host-parity.mjs`，也没有 task-5 parity manifest。 | 产品各宿主存在不等于跨宿主矩阵；验收驱动/真实请求证据仍缺。日期/报告 period 的共同 engine DTO 修复为已核销历史项，不需再另造每宿主日期算法。 |
| Todo6 (101–107) | 要原子逐 seed checkpoint、来源 revision/digest 校验、bounded resume、不能缺矩阵 finalize。当前 `scripts/simulation/baseline-run.mjs`、`.test.mjs` 都存在，含 checkpoint/resume路径；本轮不运行测试。 | 相比 Archive “未启动”已有实现进展，需按当前源与 Todo6 精确契约复核，不可以 runner 文件存在即算验收完成。不得重复开发已存在功能。 |
| Todo7 (109–115) | 要执行 fresh primary 10×30、cross-year 5×400自然日、行为/event/C01 0.5/1/2 矩阵并封存 manifest。当前找到的是旧 Task39 规模测量，不是这些新矩阵；无 Todo7 manifest。 | 尚无新鲜全矩阵证明；旧 2万/5万/10万账户日结规模不是 Todo7 seed/multiplier矩阵的替代。此为长验收且受 300000ms 共享硬期限约束，不能本轮擅跑。 |
| Todo8 (117–123) | 要 fresh default WASM/Server/Weston Tauri release surfaces 请求验证，不接受 grep。代码存在 diagnostics feature gating 和测试，但无 `release-contract.mjs` 或 task-8证据目录。 | 编译期开关/单测不是真实发行产物探针；Todo8仍未证。 |
| Todo9 (125–131) | 要仅同步已验证 platform/simulation/release 文档，并基于产物校准 clock、Wayland、body limit、C06。`docs/implementation-gaps.md:174–176` 已纠正旧 body limit / 午间 clock说法，但正式 Wayland/Plan证据链不完整。 | 当前至少两条旧陈述已主动纠正；不能沿用该计划将旧午间句或 8MiB句当现行文档事实。完整 Todo9/合同证据未见。 |
| Todo10 (133–139) | 要 `verify-plan.mjs`、clean worktree pinned revision、重建 WASM、各 manifest/receipt digest 及 gates。对应 verifier 与 task-10 seal 未找到；当前 HEAD merge `08e4fc7` 的审计树，与原计划期望的专用验证 commit 不同。 | final seal 未证明。仅有 Todo2 原始工具证据和当前产品合并提交不能取代 task-10 manifest。 |
| F1–F4 / Commit / Success (141–161) | 要独立计划复核、质量/A股语义、真实手工 QA、证据/来源核查，最后所有 APPROVE；没有这些收口产物。 | 未见四项终验。没有证据显示本计划应改变 A 股交易语义；外部校准数据缺席是已明确的边界。 |

## 工具、三宿主、clock 与日期 callers

- `corepack pnpm` 的 Todo2 证据含完整命令结果；bare `pnpm` 缺失属于预期环境测试，helper 有明确 Node24 提示。它不能证明当前任意 Node版本都能跑，当前项目仍 pin `.nvmrc`，Todo10 应复用合适 Node版本。
- 当前 `scripts/simulation/` 不是旧文档所描述的空工具目录：baseline runner/测试和诊断 divergence审计均在；但三宿主 parity、release-contract、verify-plan指定程序仍不存在。必须区分已有 baseline 实现与未实现的验收工具。
- 三宿主入口分别是生成 WASM Worker、Server HTTP/WS routes、Tauri registered invoke/actor。host adapter 或 feature-gated测试 seam 不等于真实三宿主隔离驱动；仍需每宿主真实契约与规范化状态/事件/restore 比对。
- 日内决策/诊断时间由 `packages/engine/src/session/observation_clock.rs:4–33` 统一将 tick 转为 `CivilInstant`；`decision_chain.rs` 经 `observation_civil_instant()`读取。午休分支通过 `seconds >= 7200 ? +5400 : 0` 映射，不存在旧“连续跳过午休”的缺口；因该映射是近似交易阶段模型，不把它扩展成真实交易所时间日历证明。
- 自然日状态由 `CivilClock` 保存/恢复并由 `end_civil_day` 推进；默认开局日 `2030-01-01` 是显式固定值 (`civil_clock.rs:214–215`)，Web新局 `start_date` 由 UI/setup传入，server/Tauri只转发 engine `CivilDate`。搜索当前 caller 未发现系统 wall clock (`Utc::now`/`Local::now`) 注入产品交易日期的迹象。跨宿主对外 report period 已集中为 engine 生成 `YYYY-MM-DD`，旧“仅 WASM 修复”的问题已由后续 issues 段/测试审计说明核销。

## 旧结论复核与新候选

- **需更正旧复核记录 luna37**：它称 8 MiB 为仍待 Todo4记录的当期 blocker；现产品已按 ADR-0019 使用 512 MiB + 1 MiB，请以更晚批准决策 `docs/decisions/0019-draft-market-scope-and-capacity.md:29` 和 `docs/implementation-gaps.md:174` 为准。旧 Task39原始实测仍可作为历史文件数据，但 Todo4不能再声称当前额度是 8 MiB。
- **Todo2 旧债已修复**：toolchain证据明确列 clippy和Node24 pnpm各门通过；Archive的旧 clippy债、“Todo2进行中”均是历史时态。尚欠整个Wayland计划验收，不应一并误标完成。
- **Task36 午间时钟 blocker 已核销**：共享观察时钟、决策和因果诊断同源，后续 review ACCEPT；archive/issues中早先 “clock review remains” 已被其后解决段覆盖。
- **Weston限制仍实质成立**：之前旧 PNG文件存在并不证明图像有效；未见 ≥1280×800 app pixels 和真实 IPC调用。保留独立验收债，但不要误报系统没有 Wayland支持。
- **新候选 P1（文档验收范围漂移）**：正式 Wayland计划 Todo4仍把 8 MiB mismatch作为现行目标，与更晚批准 ADR-0019、Server常量及实现 gaps文档直接矛盾。所有根据旧接受条件产出的claim checker/QA若断言必须出现“当前8MiB”会奖励错误文案，应先由计划维护者重定范围/解释历史数据。
- **未发现需要变更 A 股语义的候选**；无交易制度来源核验需要。本审查未运行任何测试、构建、Weston或性能矩阵。
