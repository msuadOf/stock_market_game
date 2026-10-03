# Luna47：公司问题、技术栈与工作状态全文复核

审计目标产品提交 `08e4fc75b52a71a3262a8a938c57b44f8b5b4960`，当前 merge HEAD `a7c7ce357bdc9f88c03633744b2d5815db49e9b2`（同产品）。已全文阅读根 `AGENTS.md` 与 `docs/principles.md`。指定全文按连续行段读至 EOF：公司 problems 95 行、tech-stack 64 行、work-status 396 行，合计 555 行。只读核对当前 caller、最新 ADR 与已有旧审计；未运行测试、构建或长任务；只新增本文件。

## 全文章节矩阵

| 文档章节/范围 | 当前调用者与原文核对 | 结论/旧结论复核 |
|---|---|---|
| `company-information-problems.md` Todo 4，7–13 | Wayland 无像素截图记录仍受限于 Weston 默认零尺寸输出；原文还称 Task 39 JSON 超过“不变的 8 MiB”远程 body gate。当前 `packages/engine/src/session/persistence.rs:944` 为 512 MiB decode limit；`apps/server/src/routes.rs:41,810,836` 将 HTTP body 上限设为该 limit + 1 MiB，并仍执行深度/总字节解码检查；`apps/server/src/lib.rs:109` 将相同 body limit 挂在 router。 | Wayland 截图欠缺仍是环境验收债。8 MiB 断言已经过时，旧审计 sweep47 已指出，但指定 problems 文件没有同步；这是本轮新增文档漂移候选，不得继续把当前 20k/50k/100k 存档说成超过现行 body gate。512 MiB 也不表示无上限或证明所有规模可部署。 |
| problems 跨宿主 report period，17–29 | 当前共享 DTO `packages/engine/src/company/query.rs:493` 由 `period_end_date` 产出 canonical date；WASM `apps/web-wasm/src/lib.rs:420`、Server/Desktop 直接返回共同 DTO，Web 的 normalizer 按 date 校验。 | 初始冲突已由后续 resolution 核销；Task 30 原始段落是历史记录，不是当前阻断。旧结论正确。 |
| problems Task 35 native dependency blocker、resolution、Wayland screenshot、retry，31–45 | 原生依赖 blocker 在第 35–37 行已自述解决；第 39–45 行说明截图限制与一次后续重试。当前 diagnostics feature/Server/Tauri actor caller 存在，`apps/server/src/routes.rs:565` 有诊断 handler。 | 构建依赖 blocker 已解决；不能将 Xvfb MockRuntime/启动证据外推成 Weston 像素或当前环境重测。真实 Wayland 图片仍未证。 |
| problems Task 34 图表残片，47–52 | 记录来自当时 Desktop production captures；本轮静态代码检索不能证明当前实物仍复现。 | 保留为历史视觉验收线索，不能标当前已修或当前仍复现；需要新截图/实物观察才能定性。 |
| problems controlled date，54–60 | `apps/web/src/App.tsx` 的输入初始化来自现有 session setup，输入值取 draft 原值；新局在 `apps/web/src/app/useSaveCommands.ts` 校验日期后才调用。 | 有效的修复记录；未发现显示层回退重现证据。 |
| problems Task 36 scope / authoritative clock，62–86 | `packages/engine/src/session/observation_clock.rs` 是共享 tick→civil instant；decision chain 与 causal collector 消费同一 clock。后续诊断因果订单关联仍另列旧 G37，不与午休时钟混为一谈。 | scope blocker 已授权解决、午休 clock 已修；保留旧段落为历史过程。不要把 G37 关联缺口因 clock 修复而核销。 |
| problems Task 1 与 Todo 2，88–95 | 文档本身把 GTK/工作区阻断标为解决，同时明确当时 Task32/35 IPC、Wayland 与 pinned Node/Corepack gate未证。 | 历史环境范围，不形成新产品实现缺口；不因旧运行器约束重开旧环境修复。 |
| `tech-stack.md` 总览及选型，1–22 | React/Vite/TS/RTK/Tauri/Rust/WASM/Axum/pnpm/Node/ts-rs/Playwright 选型与 workspace/package/Cargo caller 一致；Rust engine 供 Worker、Server、Tauri 消费。 | 选型状态不是平台运行验收声明；未发现新技术栈或越层 caller 缺口。 |
| tech-stack lint 约定，23 | 原文“CI 以 warning 为错误”；`.github/workflows/ci.yml:4,6–7` 明确只供手动开发诊断，`:218–219` 调 `pnpm --filter web lint`；`apps/web/package.json:9` 仍为裸 `oxlint`。Rust clippy 在 workflow `:212–213` 使用 `-D warnings`。 | **G26 仍成立且范围有限**：手动开发 CI 的 Web lint 有 warning 不失败；不是发布/产品构建缺 lint。新 ADR-0028 `:15–18,33–34` 明确发布仅构建，不能拿 tech-stack 文案要求把 lint 加回 Release，也不能用 Rust clippy 严格模式核销 Oxlint。沿用既有 G26，不重复编号。 |
| tech-stack 已敲定决策，27–47 | ADR-0002/-0003/-0004/-0005 与 WASM 固定 nightly、Axum/Tokio、engine 权威及 Redux 投影接线相符。 | 当前生产选型有 caller；此前的端口/类型日期问题不再由此处产生。 |
| tech-stack 待定/依据/流程，49–64 | 覆盖率门槛仍未形成 ADR；`docs/work-status.md:389` 也列明采集方案/基线待定。 | 保持待决，不应虚构百分比或据此新增代码要求。 |
| `work-status.md` 页头与三平台分发，1–95 | 日期说明和多个分发轮次均明确区分失败、局部成功与第四轮成功；当前 build/package/release caller 在 `scripts/build-targets.mjs`、`scripts/package-distributions.mjs`、`.github/workflows/distributions.yml`。 | 失败历史不应重写；第四轮 2026-10-02 九格成功优先于前三轮局部失败。GUI 安装、实际桌面旅程、签名/公证仍不由构建包验证推出。 |
| work-status ADR-0027 运行时宿主与四目标，97–138 | Web 选择本地/远程 host、WASM Worker、Axum 服务和 Tauri 的入口均存在；页面明确区分 CI、编译、browser/E2E 与平台限制。 | 旧“缺 Node/桌面尚未生成包”被后续决策和构建实证覆盖；跨平台安装/完整 E2E 限制仍须按原文范围理解。 |
| work-status 合并失败修复、ADR-0024/0025、恢复及定向记录，140–218 | 文档区分合法零成交、日终存档与恢复；当前 `ProtocolSession::restore` 拒绝活动日内订单，Web lifecycle 从一次性存档源恢复。 | 竞价测试旧假设、日内母单漏检等历史结论已修正，不重开；定向测试不等于完整回归。 |
| work-status A05 精简及 Continuous 恢复复测，220–271 | 当前 Continuous regression 使用 `snapshot_inner(true, true)` 获取全账户；低层 in-memory restore 与公共 `ProtocolSession::restore` 的约束不同。 | 旧“母单键缺失”系历史 test fixture 用了玩家投影，已由后续复测/源码修复说明更正；不可误报生产母单生命周期故障或放宽日级档约束。 |
| work-status 现行范围二次补漏，273–311 | 机构 risk latch、预算/保护价、保存 generation、结构化错误、日 K 与 inspector generation 均有当批实现记录及 caller；其红绿、编译、WASM、许可恢复等属历史证据。 | 不把同一实现重复列为新缺口；也不把旧红绿记录冒充本轮运行。G37 等独立跨层项仍按原总账，不被相邻修复核销。 |
| work-status 旧记录处理与 9/30 验收，313–337 | 明确 task37/38/40/41/42/F1–F4 未完整验收；Task39 有规模证据但缺独立闭合。Playwright 移动新增用例当时未执行。 | 对旧 scope 的核销严谨；但其验收状态是有日期的历史快照。若后续具体验收已发生，应按更晚证据补同步，而非把当时限制永久化。 |
| work-status 机构经历、授权与合成历史，341–383 | ADR-0026/0025/0023 下的实现与限制描述一致；Server token 的局部授权不等于公网账号体系；真实市场校准明确不在范围。 | 不因后来机构策略实现而重开统一止损/现金循环；同样不借局部授权闭环声称 TLS/公网认证已完成。 |
| work-status 不可冒充项，385–396 | 稳定多线程、跨日真实 host E2E、覆盖率、公网运维、签名更新、多存档和股东资金流分别是证据债、待决或明确不做。 | 分类总体仍有意义；但本页没有记录后续 `agents/main-release-validation/summary.md` 的 2026-10-03 全量回归及 test Release 实物验收（提交 `b76ece39…`、Actions `37108236778`、tag `test-20261003-075118`）。这是状态同步遗漏候选：至少远程发布/构建与该批完整回归证据应在正式状态页给出交叉引用；Pages 成功仍不等于公网完整游戏验收，不能泛化为所有 host E2E 已闭合。 |

## 旧结论复核与新发现

- G26：复核旧 `reaudit-tools.md`、`sweep47.md` 与当前 package/workflow caller，一致仍缺 Web warning-as-error，且只限手动开发 CI。ADR-0028 将发布链路改为 build-only，是较新且明确的替代决定；不把旧“Release 应跑 CI/lint”列缺口。
- 8 MiB：早期 `company-information-issues.md` 与 `work-status.md` 记载的 8 MiB body gate 已被当前 512 MiB engine decode bound + 1 MiB HTTP body margin 替代。指定 `company-information-problems.md:12–13` 仍称“不变的 8 MiB”，需要作为正式文档同步候选；过大的存档仍受有限解码与体积/性能约束，不能从 body gate 调整推断规模验收全通过。
- 最新发布证据：2026-10-03 发布与完整回归记录存在于 `agents/main-release-validation/summary.md`，指定 `docs/work-status.md` 未反映。建议状态页补充有界交叉引用，并保留该记录明确的 host/线上验收限制。
- Task34 黑色 `TV` 残片与 Wayland 截图均为视觉实证边界；本轮没新截图，不把历史记录升级成当前缺陷，也不擅自核销。
- 当前仅新增上述两个文档候选（8 MiB 陈旧断言、10/03 状态同步）。不发现新产品代码缺陷；没有改产品文档/代码，也未做测试或交易语义变更。
