# 工具链缺口复核（8cf34a1）

## 范围与判定方法

- 当前源码基线为 `8cf34a1`，对照 `b89afb3`；本记录只复核旧审计 G21、G26、G27、G39、Q05，及相关 build/CI/script 契约。没有运行全量回归、构建或 K7 矩阵，也没有修改生产代码。
- 完整阅读了 `AGENTS.md`、`docs/principles.md`、`agents/implementation-audit/implementation-audit-2026-10-02.md`、`docs/superpowers/plans/2026-09-24-single-world-multithreading.md` 和 `docs/decisions/0028-tagged-release-and-static-pages.md`。并按需核对量价清单、测试清理清单、脚本实现及其调用路径。
- “缺”表示契约问题在当前生产/工具路径仍存在；“部分”表示存在有效守卫但闭环不足；“原误判”表示旧审计错误归因或把本来允许的行为称作错误。自由并发下不同运行顺序不自动等于重放失败。

## 逐项状态

### G21 — CLI 输入投影契约：缺

- 依据：`docs/price-volume-simulation-gap-checklist.md:265-276` 明确说工具只取 `SessionSetup`，忽略的快照、委托及账户状态不参与验证；旧审计 `implementation-audit-2026-10-02.md:68` 将工具契约登记为缺口。
- 当前生产调用链：`packages/engine/examples/price_volume_baseline.rs:29-34` 将整份输入反序列化为 `SaveSlot`，随后才只消费 `slot.setup`。因此无关快照/委托字段无法反序列化时，CLI 在读取 setup 前即拒绝。调用量价报告与因果诊断的后续路径确实只使用 setup（该文件 `:33-38`），但并未落实投影读取。
- 与 `b89afb3` 对照：该 example 在两基线间无相关改动；当前差异未补上投影入口或输入边界测试。
- 判定：仍缺。这里是工具承诺与实际解码边界不一致，不代表 SaveSlot 正常业务加载应放宽验证，也不涉及交易规则。

### G26 — Web lint 将 warning 作为错误：缺

- 依据：`docs/tech-stack.md:23` 写明“CI 以 warning 为错误”；旧审计 `implementation-audit-2026-10-02.md:73` 指出 Web lint 未设置失败门槛。
- 当前调用链：`apps/web/package.json:8-10` 的 lint 命令仅为 `oxlint`；`.github/workflows/ci.yml:219-220` 直接运行 `pnpm --filter web lint`。工作区 `package.json:20` 虽以 `-D warnings` 严格运行 Rust clippy，但该选项没有传给 oxlint。当前 oxlint 配置中允许 warning 级规则，故 CI 可在 warning 存在时通过。
- 与 `b89afb3` 对照：前端 lint 命令与 CI 调用保持原样，审计基线至今无严格 warning 选项或等效配置。
- 判定：仍缺。Rust warning 门禁有效，不能据此核销 Web lint 约定。

### G27 — Release 公开前复核 tag SHA：部分

- 依据：ADR-0028 `docs/decisions/0028-tagged-release-and-static-pages.md:15-21` 要求校验冻结 SHA、拒绝 tag 移动，并在公开 Release 前核验已上传资产。
- 当前链路：`.github/workflows/release.yml:22-24` 先校验并输出 SHA；`:40-42` 等待 CI 和制品 workflow，`:49-70` 再进入发布 job。`scripts/publish-release.mjs:83-85` 只在上传前读取 tag 指向；`:86-107` 上传 draft 并核对 GitHub 资产 SHA-256；`:109` 随即把 draft 公开。公开前代码没有再次解析 tag SHA。`--verify-tag` 在 create 调用中只验证 tag 可用，不能代替与冻结 SHA 相等的复核。
- 与 `b89afb3` 对照：发布脚本及 release workflow 没有补充公开前 SHA 重查的相关差异。资产重读确实强化了制品核验，但检查对象不是 tag 指向。
- 判定：部分。上传前 SHA 检查和公开前资产核对都在；长构建/上传窗口内 tag 移动的守卫缺失。再次查询也只能缩小竞争窗口，不能冒充平台级原子不可变保证。

### G39 — K7 跨 worker 完整产物相等：缺（需求判断不变，工具契约仍过严）

- 计划依据：多线程计划开头明确无关账户/股票无需确定跨运行顺序，允许并发调度产生不同受理先后（`docs/superpowers/plans/2026-09-24-single-world-multithreading.md:3-5`）；同计划生产改造顺序要求账户争用按真实接收顺序、股票按适用价格时间规则协调，而非建立全局交易顺序（`:21-26`）。这允许独立工作并行，并不承诺整局 deterministic replay。
- 当前真实入口：`packages/engine/examples/escrow_verification_harness/committed.rs:513-518` 经 `step_frame_with_commit_evidence()` 运行正式生产 step；脚本 `scripts/simulation/run-escrow-verification-matrix.mjs:598` 调起 harness。生产受理边界 `packages/engine/src/session/pipeline/local_admission.rs:264-309` 将账户资源及股票 gate 依赖纳入局部 DAG，排队次序由真实 gate 到达登记，不构造跨独立股票的全序。
- 当前工具门禁：`run-escrow-verification-matrix.mjs:583-594` 对所有非负控模式保存首个完整 artifact vector，后续必须逐字节相等；vector 包含 state、event stream、receipts、save slot 的 hash。独立 contracts 入口 `scripts/simulation/escrow-verification-contracts.mjs:135-160` 也要求每种 worker budget/repeat 与 `1/0` 的 artifacts 和 execution coverage JSON 相等。矩阵包含 1/2/4/auto worker，故这里不是只验证同一已发生受理事实的重放，而是把跨 worker 的完整交易输出相等当作硬门禁。
- 与 `b89afb3` 对照：完整产物比较和跨预算矩阵仍在；`8cf34a1` 改动了脚本/fixture 的证据组织与校验，但没有移除这项全 artifact 相等要求。改动未使“自由并发允许不同独立受理先后”变成可重放契约。
- 判定：旧审计把“尚未冻结同一受理轨迹”列为工具遗漏，是有效发现；但不能据此推导生产必须跨 worker 产生相同整局事件顺序。当前工具仍把非确定跨账户/股票调度误判为 determinism drift，故契约缺口保留。应验证局部受理事实、资源守恒、价格时间、依赖、失败隔离等不变量；只有输入了完全相同的受理事实时，才把结果相等称为 replay 要求。不能通过删除失败负控或守恒断言来规避。

### Q05 — scripts 测试发现与覆盖策略：缺

- 依据：旧审计 `implementation-audit-2026-10-02.md:119` 登记该开放边界；`docs/test-cleanup-checklist.md` “单独发现，不并入本轮”仍明确 `scripts/**/*.test.mjs` 未由根测试及 CI 常规入口统一覆盖，要求另行盘点时限与 CI 策略。
- 当前入口：根 `package.json:13-14` 的 `test` 只进入 `scripts/run-full-regression.mjs`；Node 脚本测试的 CI 文件清单散落在 `.github/workflows/ci.yml`、`distributions.yml`（例如后者 `:77` 和 `:143`），没有 `scripts/**/*.test.mjs` 自动发现入口。普通脚本测试源码真实存在，但“某个测试被某 workflow 显式列入”不能证明其余 helper 有统一自动覆盖。
- 与 `b89afb3` 对照：`8cf34a1` 新增/修改的多个 script helpers 和对应测试提升了局部覆盖；没有更改根 `test` 的入口，也没有建立统一发现/完整性清单，因此不能核销 Q05。
- 判定：仍缺的是覆盖策略盘点与可验证完整性契约，不是断言每一个 scripts 测试都必须进入长回归。根/CI 自动扩展需兼顾 AGENTS 的 10 秒单命令约束和长验收分类。

## build / CI / script 承诺交叉检查

- ADR-0028 的“普通 commit/PR 不自动工作流”仍成立：`.github/workflows/ci.yml:1-9` 仅 `workflow_dispatch`、`workflow_call`；自动标签入口仍在 `release.yml:1-4`。本次差异没有改变 workflow trigger。
- CI 长回归仍分 build、execute 两阶段：`.github/workflows/ci.yml:187-202`；`scripts/run-full-regression.mjs:626-643` 对阶段执行外部 300000ms deadline，并保留 cleanup reserve。该契约与工作区 deadline 要求一致，静态审阅未发现本次重构取消阶段上限。
- `scripts/build-targets.mjs` 的 BuildRun 重构（当前 `:219-248`）将私有目录准备/清理封装为对象，执行路径 `:250` 以后仍在 finally 清理；ArtifactPublisher 仍先验证输出是新目录再 rename 发布（`:188-210`）。可见差异没有证据表明复用旧制品、静默覆盖输出或丢失失败清理守卫。
- `apps/web/package.json` lint warning 门禁、Release SHA race 与脚本测试发现策略分别仍如 G26/G27/Q05 所述；不将这些工具契约缺口误报成 A 股规则变化。此为静态复核，未运行测试、编译、发布或完整回归，不能声称当前 workflow 的运行结果通过。
