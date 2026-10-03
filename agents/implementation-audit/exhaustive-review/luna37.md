# Luna37：阻断 / Wayland 全文独立复核

审查基线：产品提交 `08e4fc7`（题定 merge 同）；工作树 `HEAD=a7c7ce3`。只读追溯 `.omo/notepads/company-information-npc-intentions/problems.md`、`.omo/plans/resolve-blockers-wayland.md` 及其正式全文计划 `docs/superpowers/plans/2026-09-13-resolve-blockers-wayland.md`，并核实当前源码入口、文档和证据目录。目标文件分别 78、64、161 行，均从首行读至 EOF。项目守则与 `docs/principles.md` 已读；本轮不涉及 A 股规则或产品代码。

## 逐章矩阵

| 文档章节/行 | 历史结论复核与当前证据 | 当前判定 |
|---|---|---|
| problems.md:9–14，跨宿主 report period 原问题 | 下文 16–21 已记载 engine 集中输出 `YYYY-MM-DD` 的修复；此日志条目是已被自身后续段落解决的历史，而非当前 blocker。 | 旧问题已核销；不应再要求单独 WASM 日期运算。 |
| problems.md:16–21，report period resolution | 记录称 engine DTO 为 WASM、server、desktop 共用；本次指定扫描的 Wayland计划没有重开此接口问题。 | 保留历史修复证据；未从此条推导 Wayland验收。 |
| problems.md:23–29，Task 35 native dependency blocker | 原始缺失 GTK/WebKitGTK pkg-config 资料及后续授权安装、MockRuntime/Xvfb 测试、Wry Xvfb 启动记录相邻。Task-35 raw/pre/post-install 证据文件存在。 | 环境构建阻断已解决；Xvfb 的 actor / launch 证据不等于 Weston像素或注册 invoke 的手动验收。 |
| problems.md:31–37，Wayland截图与 blocker retry | Wayland 行只证明历史 Weston 与 Wry 建立 socket / launch，截图失败；紧随 retry 的 native crate blocker 后来由 27–29 明确解决。`task-35-wayland-screen.png`、`task-35-wayland-info.txt`、`task-35-weston-help.txt` 存在，但没有当前收口计划产出的 `task-3-wayland.*`。 | Wayland视觉缺口仍有效；不可把 retry 已解决的 GTK blocker 错报为当前缺环境，也不可把 PNG 文件存在误报为有效画面。 |
| problems.md:39–44，Task 34 chart watermark | 明确归 Task 34 chart surface，和本计划 Wayland blocker 不同；正式历史问题文件仍记录此 UI artifact。 | 与本轮无关，不把它静默并入 Wayland修复范围。 |
| problems.md:46–52，Task 34 controlled date | 后续段落记有移除显示 fallback、E2E/独立视觉复核；与Wayland计划无依赖。 | 已记为修复，不是本批遗漏。 |
| problems.md:54–68，Task 36 causal scope / lunch clock | 后续 70–78 记载授权后用共享 tick-to-civil clock 修正、回归通过及 reviewer ACCEPT；末尾仍请求 primary orchestrator final acceptance。Wayland计划 Todo 1 本身要求补 Task36 receipt。 | 时钟问题已有独立修复审查；Task36 最终主验收收据属于计划证据债，不可误写成现行时钟行为故障。 |
| problems.md EOF:70–78 | 明示“no checkbox changed”，说明收据/核销状态与源行为修复分离。 | 维持此区分。 |
| `.omo/plans/resolve-blockers-wayland.md:1–5`，模板状态 | 本地薄模板已明确退出当前待办并导向正式副本；`docs/superpowers/README.md:7–11` 也说 Wayland计划归档时尚未执行、属于进行中工作。 | 只能视为非执行模板；正式副本不能因此视为已取消/完成。 |
| `.omo/...resolve-blockers-wayland.md:7–64`，模板章节全段 | TL;DR、Must-have、验证策略、波次、依赖表、占位 Todo、F1–F4、提交策略和验收全是模板占位。`docs/work-status.md:322` 也明确该模板不作执行任务。 | 不从占位文字派生新工作或状态；以 161 行正式计划为审查对象。 |
| 正式计划:1–21，TL;DR | 承诺 Weston原生像素+IPC、宿主 parity、fresh resumable K7、release/doc/final gates；非产品规格，也明确不改交易制度。 | 历史计划仍是实际剩余验收的范围声明；非当前已通过证明。 |
| 正式计划:23–34，范围与验证 | 要求 ≥1280宽真实 Wayland capture/IPC、真实多宿主、fresh matrix、clean pinned build；禁止假图、旧 WASM、grep-only proof、减矩阵。 | 接受口径仍具体；目前 resolve-blockers 证据目录仅有 Todo2 记录，不能宣称计划验收闭合。 |
| 正式计划:36–56，waves / dependency matrix | Todo1–10 依赖图有序，Todo3 阻塞 Todo5/8/10，Todo6→7→9→10。 | 依赖逻辑未见缺项；这只是计划流程，非实现状态。 |
| Todo1，正式计划:61–67 | 要逐审 31/32/35 并补 Task36 acceptance、clean pinned worktree / workspace gate。当前 `agents/implementation-audit/` 有历史逐项扫描，`.omo/evidence/resolve-blockers-wayland/` 没 task-1 receipt。 | 证据收口债。旧 Task35 GTK blocker 不能代替 Todo1 receipt。 |
| Todo2，正式计划:69–75 | 目标 Corepack、clippy 和 web gates；唯一当前 `resolve-blockers-wayland` 证据是 `task-2-toolchain.txt`。当前 repo 下有多个 pnpm/Node 调用者，严格入口与 clippy gate应依原验收记录核，不从工具脚本同名/旧记录存在推定当前通过。 | 本审查不重跑也不宣布通过；需主审核对 task2 原始命令 / exit evidence。 |
| Todo3，正式计划:77–83 | Task35 只有 Weston默认 headless launch/失败截图记录；Todo3要求 bounded renderer×dimension probe、≥1280×800、decoded nontrivial app pixels、Wayland下 actor/IPC tests 与清进程。 | 明确未满足的环境验收债，不是产品实现缺少 Wayland支持的结论。 |
| Todo4，正式计划:85–91 | 8 MiB body-limit影响、Wayland适用边界及 primary-source限制要进具名文档；plan reference 的“8 MiB”是需复核目标，不足以证明 limit 已改或遗漏交易校准。 | 文档合同债；不得扩大成提升限制或外部市场校准要求。 |
| Todo5，正式计划:93–99 | 要求 generated WASM worker、鉴权 server HTTP/WS、真实 Weston Tauri IPC driver。当前 server / desktop 的 `host-parity` feature确有测试路由与actor测试（例如 `apps/server/src/lib.rs:96–102`、`apps/server/tests/api_contract.rs`；`apps/desktop/src-tauri/src/lib_tests.rs:168–198`），但 `scripts/simulation/host-parity.mjs` 和对应真实host驱动不存在。 | 不可因 test-only seam 存在报 Todo5 通过；缺集成 driver / 运行证据是计划级缺口。 |
| Todo6，正式计划:101–107 | 计划要求checkpoint与resume；当前 `scripts/simulation/baseline-run.mjs` 和 `.test.mjs` 已存在，测试包含 resume / atomic checkpoint路径。不要因旧计划点名脚本而报告脚本缺失；需比对现在CLI/API是否满足本计划的精确spec、digest/revision、CLI bounded batch。 | 历史实现有实质后续，不机械报“缺脚本”；此审查未运行测试，具体 acceptance仍待 Todo6 receipt。 |
| Todo7，正式计划:109–115 | 要 full primary/cross-year/sensitivity matrix 和新鲜、完整manifest。当前 `scripts/simulation/baseline-run.mjs` 有 after/sensitivity 调用者，但 `.omo/evidence/resolve-blockers-wayland` 无 task-7 manifest；仅旧 Task39材料不能证明 fresh after matrix。 | 完整矩阵未证；不能用 runner实现存在、seed片段或 before基线取代执行。长验收需依项目硬上限另行分类。 |
| Todo8，正式计划:117–123 | 计划明确要求 release-contract.mjs，对真实默认 release WASM/Server/Weston探测；当前脚本不存在。源代码的 feature gating / release absence test 不等价真实产物和请求探针。 | 当前可证的是隔离源码/测试覆盖，计划声明的全表面 release verifier及证据仍缺。 |
| Todo9，正式计划:125–131 | 文档需基于产物更正文档时钟句、声明Wayland边界/body limit/C06缺项；当前 `README.md:163–179` 记录Wayland headless有限适用范围，但启动命令没有尺寸、截图、IPC步骤。 | README已有边界声明；不是Todo9完整交付证明，也没有扩写用户文档的依据，除非其余验收先产出。 |
| Todo10，正式计划:133–139 | clean checkout、重建产物、digest manifest、CI等所有 gate；`scripts/simulation/verify-plan.mjs` 不存在，resolve-blockers evidence目录缺该封存物。 | final seal仍缺。 |
| F1–F4 / commit / success，正式计划:141–161 | 要独立合规、质量/A股语义、真实手验及证据复核；要求F1–F4 APPROVE。存档归档表有“尚未执行”快照；不能把普通 source审计、Xvfb测试或历史图片当F3。 | 终验未证，且与Todo3/5/7/8/10的证据依赖一致。没有发现扩大A股语义或改交易规则的必要。 |

## 旧结论复核与新观察

- 旧结论“GTK/WebKitGTK构建阻断已解决”正确，但必须限定在包安装后 feature/default checks、MockRuntime和Xvfb；不能外推到Wayland像素/GUI注册命令验收。
- 旧结论“Wayland launch有效、screenshot无效”正确。实际 README caller（`README.md:166–177`）仅创建 runtime dir、启动 Weston、运行带 `simulation-diagnostics` 的 app、检查 `wayland-info`，没有指定尺寸、renderer、capture client、像素断言、真实Tauri invoke操作或进程树 supervisor；这条是安装/启动说明，不能伪装成验收命令。
- 新遗漏 / 需限定：task-3计划写“stale generation/no-record tests pass under Weston”，但此类 actor测试本质上由 Tauri MockRuntime actor queue驱动；若未明确把UI真实IPC操作和actor单测分开，这个句子易让人误读单测由 Weston提供。验收应分别保留 MockRuntime协议断言、Wayland真实界面画面、真实注册 `invoke` dispatch 证据。现有证据只确认为 MockRuntime/Xvfb和真实 Wry launch，不确认 Weston dispatch。
- 当前 host-parity routes/tests 是编译期开关的测试工具，不是生产协议。查到的源码 caller 不构成 Todo5 的 standalone driver，也不应当作正常 release API。
- `scripts/simulation/host-parity.mjs`、`release-contract.mjs`、`verify-plan.mjs`、`task-3-wayland.*` 在本扫描基线没有；而 `baseline-run.*` 存在且有现行调用测试。前者可按计划报告未交付，后者必须按行为/证据复核，不能同名机械报缺。
- 文件名虽仍保留 company-information 主题，正文和 `docs/superpowers/README.md` 明确将计划归档为在途；工作状态 `docs/work-status.md:322` 只将薄模板定性为非任务。混同模板与正式副本会错误地把未做验收结论成已取消。

## 复核范围限制

本次没有运行测试、启动 Weston、构建桌面或检查线上状态；不为计划中的大矩阵制造新执行。没有找到需修正交易语义的依据。发现均是历史状态限定及缺少任务验收证据，不主张补产品代码。
