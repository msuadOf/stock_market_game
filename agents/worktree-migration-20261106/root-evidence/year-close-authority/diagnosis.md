# 年末权威链失败诊断（2026-10-25）

## 冻结输入与原红

- 原 source：`.worktree/integration-117-118-121-124-126` 的 full9 sealed inventory；1679 个文件逐一校验 SHA-256 后精确复制到独占 `.worktree/year-close-diagnosis`。准备耗时 4.128s，外部共享 300000ms deadline 包含创建 worktree、复制和验证；没有复制或写入正式 build cache。
- 原 binary：full6 artifacts 的 `company_scenarios-905a9d1fb08bb870`，SHA-256 `8883654dcfcedb88d189c817b20a153f2e89ed9840d2a7792f3b112adb78d81c`，与 full9 inventory 一致。
- 原 required-long 单独执行（`--exact lifecycle::year_boundary_keeps_company_operations_and_disclosure_state_authoritative --ignored --nocapture --test-threads=1`），外部 300000ms deadline；3.14s、exit101，同 lifecycle.rs:102 的 Annual versions 严格增加断言失败。完整原日志在仓库根 `.tmp/year-close-authority/original-red.log`，绑定在 `original-binding.json`。

## 公共 API 对照

探针复用原 focused fixture 和控制 plans/attention 的步骤，以只读正式 `libengine-9aba1e99418d9a84.rlib` 构建独立 executable。该 rlib 的 SHA-256 为 `76b29b322fa16901cb9c2948f51fde9864e8d2640c667d3a2eccddf322b74b64`（探针仅为诊断，最终交付会另做源构建绑定）。rustc 使用 128 codegen units，受独立 300000ms build deadline；运行时两臂独立并行，分别受 10000ms 进程树 deadline。原始 source、构建日志、runtime/legacy 日志在根 `.tmp/year-close-authority/probe*`。

| 观察 | 全默认目录 C-600101 | 实际 focused 目录 C-600101 |
|---|---:|---:|
| CompanyId | 3 | 0 |
| step 前 Annual versions（2030-12） | 0 | 0 |
| step 后 Annual versions（2030-12） | 0 | 0 |
| explicit end_civil_day 后 Annual versions（2030-12） | 0 | 1 |

runtime=true 与 ENGINE_STEP_RUNTIME=0 均为上述结果。step 前后 civil date 均 2030-12-31；显式 end_civil_day 后 civil date=2031-01-01，operations.next_expected_date 同步 2031-01-01。实际 catalog 仅含 C-600101，数值 id=0。

## 输入 → 错误结果与最小方案

`CompanyCatalog` 按当前目录中 code 排序分配 CompanyId（`company/spec.rs`）；`CompanyOperations::assemble_new_game` 仅按本局 issuer 清单装配 registry，再将该 registry.catalog 放入 operations（`company/assembly.rs:107`）。focused fixture 仅保留 600101。既有 required-long 从独立九公司默认目录取 id=3，随后在只含 id=0 的实际 closing_registry 与 public_report query 上查 id=3，导致把正确关闭的实际公司视为未结账。该错误在 runtime 和 legacy 均重现。

修复仅在 required-long 中从现有 `controlled.company_operations.catalog()` 解析 CompanyId。保留 closing_before 在 step 之后的原时点、Annual versions 的 `>`、显式 end_civil_day、required-long 分类、完整同一 Session 年末到年度披露循环及最后 save/public report 强断言。不改 production、披露窗口、日历、会计或交易制度。原始 required-long 红证先于实现；短诊断已有真实 close 公共行为，不另加只有取 id/反查 code 的镜像测试。

## 语义依据与适用边界

适用检查日期 2026-10-25。本改动只修测试对本局 CompanyId 的来源，遵守既有数值身份与单一权威源契约；未调整交易制度或真实结账/披露规则。依据 ADR-0025 的自然日日终顺序、ADR-0035 的共同披露/身份契约、`docs/simulation-calendar.md` 的自然日/市场双时钟和 `docs/company-accounting.md` §5 已登记的游戏报告排期。本次没有重新核验其外部法源，不扩称当前官方规则已复核，也不修改任何已登记的简化。

独立方案审核：`/root/sol61_proc_exit_review` 已亲读两臂原始证据与分配/装配链，认可只修本用例身份来源；最终完整增量与年末到披露绿仍待复核。

## 定向实现与构建

最终增量仅 `packages/engine/tests/company_scenarios/lifecycle.rs` 的 CompanyId 查询来源。`scenario_catalog` helper 保留，不顺修其它 fixture。

第一次构建前的 source snapshot 脚本误用 worktree cwd，FileNotFoundError；其后同 shell 仍构建了未绑定副本 `company_scenarios-fixed`。该副本不用于交付证据。随后重新执行独立 build-and-bind：构建前记录 1679 source 文件和 978 个只读库候选依赖（rlib/rmeta/so/native a）的完整 SHA，fresh rustc 编译新副本 `company_scenarios-fixed-bound`，构建后逐项复核 source 和 dependencies 均不漂移。绑定记录 `fixed-build-binding.json`；128 个 codegen units，采样已观察多个 opt cgu 线程，编译 2.653s，含前后校验全阶段 13.896s。最终 binary SHA-256 `e4af98b5a733a30797d260b53dacc43319739abe6c9f71a336e92d1d70dd1117`。

原 required-long 在该实际新 binary 中原参数完整通过，4.17s，年度报告为 2030-12，2031-03-25 发布，versions=1。原年度链的其它断言全部保留并执行。

8 个普通回归按独立 10000ms supervisor、8 个进程并发执行，总 wall=6.408s；两项 fixture + 5 项 legacy 真实链通过，legacy 的 `civil_information_chain_keeps_unread_beliefs_stable_and_closed_days_tick_free` 在 2.90s 报错（正在用原冻结 binary 对照归因）；不能声称本轮所有定向短测全绿。runtime 下该例已有 return guard，本次未修改。

## 最终运行绑定与限制

补足执行 receipt 后又以同一已绑定 binary、同一 required-long 精确用例执行一次（未更改测试或 source）。`fixed-year-run-input.json` 在 child 启动前发布实际命令、binary SHA、ENGINE_STEP_RUNTIME=1、harness_threads=1 与 300000ms 外部共享进程树 deadline（1000ms cleanup reserve）；`fixed-year-run-result.json` 记录 exit0、child wall=4.607s、包含 source/binary 前后校验及 child wait 的本脚本 wall=5.697s。测试 harness 自报 4.20s，输出仍是 report14、period2030-12、published2031-03-25、versions1。`fixed-year-cpu.json` 保存运行中的真实进程线程采样。同一 Session 的自然日依赖链必须顺序，runtime 内部仍使用其 NPC 组与交易部线程；不把固定一 harness thread 称为单线程引擎。

legacy 对照已收口：原 SHA8883654d… binary、ENGINE_STEP_RUNTIME=0、相同精确短例与独立10000ms外门，同样在 lifecycle.rs:46 beliefs字节相等处失败，harness2.72s/exit101。证据 `original-legacy-short-control.log`。因此本补丁只修 required-long 的目录身份错域；既有 legacy 短链失败明确保留，不宣称所有相关测试全绿。full9 的正式 runtime 3713 普通通过不等于该 legacy 链通过。

交付 patch：根 `.tmp/year-close-authority/year-close-identity.patch`，SHA-256 `c4dfc783af6865416fddec903409af80f845b7ef82f00fe0751a7c94af48b503`；改后 lifecycle source SHA-256 `212c050a393303964a6a3b50475261c7e98ede4927a40ddcae043c544fc22109`。增量仅此一个 source 文件。原始红证、未绑定的初次构建及全部失败日志均保留；没有写入集成 worktree、正式 cache，没有运行完整回归，没有提交或 push。

独立终审：`/root/sol61_proc_exit_review` 于本次最终冻结后明确 APPROVED，仅授权上述单文件 patch/source 集成；审查记录 `agents/year-close-authority/sol-review.md`。此结论不代表完整回归或整树通过，既有 legacy 失败边界不变。
