# Sweep37：历史阻断、Wayland 模板与协作门禁

本轮仅静态审计、新增此记录；未改产品、未执行 Git 写操作、未运行测试或环境安装。工作树读取基线沿用 `4ad5a2e`，主控说明产品与指定 `b76ece3` merge 相同。

## 全文读取

| 文档 | 行数 | 覆盖 |
|---|---:|---|
| `.omo/notepads/company-information-npc-intentions/problems.md` | 78 | 连续 1–78，全部问题与追加 Resolution |
| `.omo/plans/resolve-blockers-wayland.md` | 64 | 连续 1–64，含顶部核销说明、模板 Todo/F1–F4 |
| `AGENTS.md` | 123 | 连续 1–123，全部约束与导航 |
| `docs/superpowers/plans/2026-09-13-resolve-blockers-wayland.md` | 161 | 连续 1–161，作为模板实际计划指针补读，全部 Todo1–10/F1–F4 |

指定三文合计 265 行，含补读实际计划合计 426 行。已读 `docs/principles.md`，此前同轮已核对决策与 `docs/open-questions.md`。

## Problems 全部章节

| 原文章节与行号 | 当前状态 | 当前代码/反证与限度 |
|---|---|---|
| Cross-host period :9，Resolution :16 | 历史源码缺陷已修 | `packages/engine/src/company/query.rs:493–495` 公共 DTO 的 period 经 period_end_date→ISO 日期；Server `apps/server/src/actor.rs:788`、Desktop `apps/desktop/src-tauri/src/lib.rs:178`、WASM `apps/web-wasm/src/lib.rs:420` 共享 PublicReportSummary。不能沿用旧 YYYY-MM 公共契约缺口。内部 ReportSet 的 AccountingPeriod 仍是月份不构成公共格式错误。 |
| Task35 native libs blocker :23，Resolution :27，retry :35 | 历史环境阻断；已有修复声明，当前未重新验收 | 追加记录明确原 native 阻断已解除；后文同日期 retry 再次缺库，不能以段落顺序推断当前机器已装/仍缺。当前 actor 生产接线与 generation 检查存在，`actor.rs:1122`；`:1310/:1324` stale 测试、`:1340` release unsupported、`:1363` feature supported 测试存在。本轮未构建，故不声称当前 native check 通过或失败。 |
| Task35 Wayland screenshot :31 | 视觉证据债保留 | 原文只证明 compositor/Wry 启动，screenshooter 失败不能算像素验收。`README.md:163–177` 有真实 Wayland 启动命令，但未设置 ≥1280×800 renderer/capture validator；`scripts/desktop/build-matrix.mjs` 是构建矩阵，不是实际 Wayland UI/IPC 验收。总账 :177、R20 已保留此债。 |
| Task34 TV artifact :39 | 历史图表视觉候选，当前是否重现未证明 | `apps/web/src/components/price-chart-runtime.ts:52–66` 仍创建 lightweight-charts 主图，未找到显式 watermark 修复或 attributionLogo 配置。但这不能仅凭代码证明仍有 detached black TV 像素；也不能把隐藏 logo 当成唯一正确修复。需当前 Desktop 像素复核，纳入已有视觉债，不新增确定 G 缺陷。 |
| Task34 controlled date :46 | 已修复，有当前代码反证 | `apps/web/src/components/StartDateInput.tsx:24` 直接 value={value}，`:13/:26/:31` 同一值的验证与显式错误。`apps/web/src/App.tsx:149` 仅初始化 draft；`apps/web/src/app/useSaveCommands.ts:212` 新局解析 draft。不存在旧 value \|\| DEFAULT_START_DATE 展示 fallback。 |
| Task36 diagnostics source scope :54，permission resolution :58 | 限制已解除，生产 collector 已在 | `packages/engine/src/session/causal.rs:28` 捕获实际观察时钟；`session/decision_chain.rs:753–763` feature-only plan diagnostics 接入真实根结果。不能把旧仅准 diagnostics 文件的委派限制当成现行产品无 collector。当前诊断订单关联缺口另为既有 G37，不因原记录称 hooks 已在而核销。 |
| Task36 lunch clock :58，Resolution :70 | 时钟已修，最终批准债与代码分开 | `session/observation_clock.rs:27` 下午累计加入 5400 秒午休；生产 `decision_chain.rs:74` 与 `causal.rs:28` 共用 observation_civil_instant。`:3389/:3407` 午休回归测试存在。旧 chain_observation_instant 已成为 `#[cfg(test)]` 代理 `:741–743`。未重跑测试；历史最终 Task36 re-review 请求是证据收尾，不重新登记午休漏实现。 |

## 未填充 Wayland 模板逐章

顶部 `.omo/plans/resolve-blockers-wayland.md:3–5` 明确模板退出待办，实际计划见正式路径，不声明环境验收完成。因此 TL;DR :7、Scope :26、Verification :30、Execution :35、Dependency :39、Todos :43、Final :53、Commit :60、Success :64 中 `<title>`/`<...>` 都是已核销模板；不能逐个登记「未实现功能」，也不能用模板核销推出具体 Wayland/K7 已完成。

## 实际 Wayland 计划各项与正式工具

| 原文任务 | 状态 | 代码与证据界限 |
|---|---|---|
| Todo1 reconciliation :61 | 部分历史验收已有，完整最终 receipt 待证据 | 总账引用 `agents/oop-release-validation/summary.md` 是后续完整回归/fresh 浏览器/build-only 发布历史证据；不推出三宿主真实日旅程、Wayland/F1–F4 全通过。 |
| Todo2 Corepack/clippy :69 | 正式入口已有；本轮未运行 | `scripts/corepack-pnpm.sh` 存在；普通 Web 测试入口 `scripts/run-web-tests.mjs:145–166` 有外部 supervisor，内部启动绕过时拒绝。旧工具链阻断不是当前缺代码证据；独立 lint warning 缺口仍为既有 G26。 |
| Todo3 Wayland UI/IPC :77 | 正式视觉验收尚无本轮可核销证据 | README 仅启动说明；未找到实际 `scripts` Wayland 像素/IPC verifier。符合已有 R20/总账长期 GUI 债，不把 actor 单测冒充 Wry 渲染证明。 |
| Todo4 limits docs :85 | 原 8MiB 问题已被后续修正，范围债不回退 | 总账 :156 明确修正。实际远程上限沿现行 host 核对，不能执行旧计划「不得提高限制」来复活已被后续范围取代的数量限制。 |
| Todo5 three-host parity :93 | 生产 hosts 已有；完整验收债 | 历史命名 `scripts/simulation/host-parity.mjs` 当前不存在。未找到同名不自动证明产品 hosts 没实现；Server/Desktop/WASM query/actor 当前已在。要求的真实 Web/HTTP+WS/Weston IPC 完整矩阵仍为总账 :177 保留项。 |
| Todo6 resumable K7 :101 | Runner 已实现 | `scripts/simulation/baseline-run.mjs:941–959` 校验 checkpoint schema/identity/digest/seed/provenance/argv；`:1199` 拒绝重复/多余 seeds finalize；`:24–25` child 与共享批次均 300000ms。正式 artifact verifier `verify-simulation-artifacts.mjs:97/:139/:147/:156` 有 missing/artifact-set/git/resource policy 校验。 |
| Todo7 complete fresh matrices :109 | 执行/密封验收债，非仅 runner 代码债 | `baseline-run.mjs:1412/:1414` incomplete 不能 final；`:1416` manifest 明示 C06 synthetic-only。工具具备不证明所要求的完整矩阵真实跑完；G39 错误跨 worker 全产物相等入口由既有 tools 核对保留。 |
| Todo8 release surfaces :117 | 部分正式工具与后续发布证据已有；GUI 不扩大结论 | `scripts/check-web-release-wasm.mjs` 等正式 release 检查存在；Desktop actor `:1340` no-record unsupported 测试存在。同名 `release-contract.mjs` 未找到，未据此新增产品缺口；真实 Weston release IPC 仍属既有最终验收债。 |
| Todo9 docs :125 | 现行正式文档与旧漂移分别核对 | 总账 :183 已登记 causal 文档旧午休描述与 feature 命令漂移；不重复计数成新产品漏实现。 |
| Todo10 seal :133 | 部分正式验证工具已在；完整封门不能核销 | 同名 `verify-plan.mjs` 未找到；当前有 `run-full-regression.mjs`、`verify-simulation-artifacts.mjs`、build/distribution 工具。现有具体 artifact 验证不能代替完整 pinned source 下的全部 GUI/K7/F1–F4。 |
| F1–F4 :141、commit/success :152/:157 | 全部为验收/证据要求 | 不以 historical checkbox、MockRuntime、源码有脚本或 build-only 发行推出实际像素/跨宿主/矩阵全部通过；本轮不执行外发。 |

## AGENTS 全章约束核对

| 章节 | 本轮状态与相关实际实现 |
|---|---|
| 项目/三铁律 :9/:21 | 审计追 engine/web/server/desktop 的实际 caller；未改运行语义。历史测试存在只算测试覆盖，不编造 TDD 执行历史。 |
| 大 A/独立复核 :38 | 午休以现有共享 civil clock 反证；无新增规则决定。本记录待主控统一独立 diff 审查，不自称完成产品门禁。 |
| checklist :53 | 已读原则、相关决定和开放问题；无依赖、Git 流程或开放政策变更。 |
| 工作文件 :62 | 唯一新增文件位于 agents/implementation-audit/exhaustive-review，未污染正式 docs。 |
| 多核与 deadline :69 | `run-with-deadline.mjs:6–8/:29` 明确 10000/300000/1000 上限；`run-web-tests.mjs:54` CPU 分片，子 test-concurrency=1 位于每独立 shard，并非整命令单核；`:151` 外部 deadline。工具存在不宣称所有长用例已实测达标。 |
| Conventional Commits/GitHub/绝对不要 :81/:97/:102 | 未 commit、push、建 PR、删弱断言、静默修历史错误。 |
| 导航 :112 | 仅工作规范导航，不把链接当成待产品实现功能。 |

## 新候选与总账去重

未找到需要新登记为确定产品实现缺口的条目。保留两个具体证据索引：历史 TV detached fragments 的现行像素重现未知；Wayland 像素/IPC、三宿主/K7完整封门仍缺本轮核销证据，均归现有总账 :177/R20。不存在三个旧脚本名是「历史指定验收工具尚不能逐名证明交付」的证据，不足以绕开后续替代正式工具，重新增加三个漏产品能力。

现行 period 日期、控制日期无 fallback、午休源时钟、feature-only collector、Factory/actor generation 都有当前生产代码反证。历史 resolved 文本只提供线索，本结论从当前 caller 得出；native/GUI 是否当前真实运行成功仍未检验。
