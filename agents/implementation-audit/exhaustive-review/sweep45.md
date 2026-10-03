# sweep45：company-information 归档与 Wayland 收口计划

## 全文范围与结论

- 连续全文读取 `docs/superpowers/2026-09-13-company-information-archive.md`（122 行）、`docs/superpowers/README.md`（11 行）、`docs/superpowers/plans/2026-09-13-resolve-blockers-wayland.md`（161 行），合计 294 行。全文包含归档修改/未提交内容提示、全部 Todo 与 F1–F4，没有以复选框判完成。
- 对照 `b76ece3` 合并同等产品、现行总账和工具/宿主复核，继续检索实际脚本、生产调用和后续批准范围；没有产品改动、Git 写操作或测试运行。本批没有确认可新增的生产 G。
- 三宿主完整 parity、Weston 像素/实际 IPC、全表面 release 与总封存验收仍需证据，旧总账已在 R20/验收债及 `docs/implementation-gaps.md:160`、`:163` 登记。`host-parity.mjs`、`release-contract.mjs`、`verify-plan.mjs` 当前未找到；局部 helper、feature 入口、短测试和一般打包不能替代这三个完整驱动。

## archive：122 行逐章节

| 原文位置/条款 | 当前事实与后续修正 | 状态 |
|---|---|---|
| `2026-09-13-company-information-archive.md:3` 原工作区即将删除、归档不是规则权威 | `.omo/evidence/` 当前仍能读取历史 review；`docs/superpowers/README.md:3` 明确后续 ADR/现行实现优先。 | 归档保存不创造新产品功能，不删除历史证据 |
| `archive.md:8` 总览 37/46、Wayland 0 项和 HEAD 记录 | 当前生产公司/披露/计划/宿主骨架存在，但这些旧计数和 a36ef84 不是当前完成度。既有 G06–G09/G28/G35–G38 已记录其中断链。 | 历史状态，不能因旧「37 完成」核销局部缺口 |
| `archive.md:23` W1–W6 已完成能力及227证据文件 | 与正式 `session` 公司经营/个人 root、`SaveSlot`、三宿主查询和 DEV 边界交叉；四行业/集团生产漏接已由 G28/G36 登记，经历由 G08 登记。 | 主要模块存在；原模块绿不等于完整生产闭环 |
| `archive.md:41` 逐项回执：REJECT/延期/qualification | `task-28-review.md:9` 原文本已区分生产路径存在与 scenario 证明不足；`specs/2026-09-13-company-information-issues.md:78` 后续 task29 civil/disclosure APPROVE；当前 Web `save/schema/company/books/index.ts:9` 分行业深校验，Industrial/Bank/Insurance/RealEstate 都有对应 parser。 | 不沿用旧REJECT判断当前代码仍裸透传；回执是否闭合另属验收记录 |
| `archive.md:49` task28 修正、`:50` task29 深 schema、`:51` task18 延期、`:52` task39 缺 review | 现有公司场景/恢复/诊断测试是静态可读实现；本批没有重跑或补造历史独立签字。task18 估值现行差错仍按 G06，不能用审查延期取代具体公式证据。 | 有已修生产能力和仍待验收门禁，二者分开 |
| `archive.md:53` task34 条件满足、`:54` 31/32/35 实际 IPC/视觉保留 | 实际 Tauri invoke command 在 `apps/desktop/src-tauri/src/lib.rs:186` 等处登记，仍不能从命令定义推导 Weston 调用及像素通过。当前 `scripts/desktop/build-matrix.mjs` 是构建入口。 | 保留 GUI/IPC 完整验收债，不等于 Tauri 完全没有入口 |
| `archive.md:60` 并发执行中的 Todo1–4、`:64` 主计划未完成9项 | 后续源码工具有 Corepack helper、K7 resume/root verifier、release WASM 导出守卫；旧0/进行中状态不证明这些现在未实现。Wayland/parity/final-review证据仍需独立核验。 | 逐任务具体判定见下表 |
| `archive.md:76` task31 civil failure/restore/resync/revision | Server `actor.rs:1214` 回滚并 stop_with_failure，`:1223` 明确故障；`:1364` restore 更新 timeline generation/public revision；`routes.rs:1417` resync 安装新 baseline cursor，`:1423` 恢复交付；`publisher.rs:76` 校验 timeline generation。 | 旧关键失败吞错/时间线问题已有后续生产修正；G01–G05当前宿主边界仍独立保留 |
| `archive.md:81` 8 MiB，`:83` Weston，`:85` C06，`:86` clippy，`:87` 水印，`:88` 简化清单 | `apps/server/src/routes.rs:41` 当前为 engine 512 MiB+1 MiB；`docs/diagnostics.md:62` C06 明确不在产品范围；`behavior/decision.rs:228` 原 unnecessary_filter_map 现为 map；README:163 明示 Wayland与X11边界。 | 8MiB/C06等待授权/旧clippy不是现行漏项；像素、水印仍需视觉核对，不能静态声称消除 |
| `archive.md:91` 归档映射、数据留存与未提交副本、`:105` 注意事项 | 三笔记和两计划副本仍在历史 docs 目录；`.omo/evidence` 保留，不因已退役可执行复验而删除原始证据。正式 `docs/test-cleanup-checklist.md:88` 明确用户接受旧证据不可执行复验。 | 档案保留/来源纪律，不要求复制同一证据到产品状态 |
| `archive.md:118` 后续wave及双F门禁 | 没有将旧全任务签字写成当前已通过；本批仅静态覆盖。 | 验收债，不新增生产行为 |

## README：11 行全文

`docs/superpowers/README.md:3` 的历史假设警告和 `:5` 的正式规则优先适用本批所有判定；不能从旧计划复活 T+0、可配置涨跌幅、旧存档兼容或已经取消的机器条数配额。`:7`–`:11` 的 Wayland「尚未执行」是归档时状态，须沿后续工具与实际证据更新理解，不能当作当前源码全部缺失的结论。

## resolve-blockers-wayland：161 行全部章节与任务

| 原文位置/任务 | 当前代码证据、后续决定 | 状态 |
|---|---|---|
| `resolve-blockers-wayland.md:3` TL;DR，`:23` Scope，`:31` Verification，`:36` waves/dependencies | 规定像素与真正IPC/HTTP/WS，不用活进程或grep代替；README:163 已登记Wayland，`:179` Xvfb只fallback。并行wave是该历史实施组织，不是当前产品协议。 | 验收口径仍有效，具体实现范围以下逐项判 |
| `plan.md:61` Todo1：对账/独立回执/干净固定版本 | 历史 task31/32/35/36 回执及归档§2a有qualification；当前 root regression `scripts/run-full-regression.mjs` 与 `docs/testing.md:102`–`:117` 记录密封 source/binary inventory，静态存在不证明本批 pinned head 全验收通过。 | 正式回归工具已有，历史回执/全门禁仍须证据 |
| `plan.md:69` Todo2：Corepack/Clippy | `scripts/corepack-pnpm.sh:21` 验 Node pin，`:27` Corepack错误带下一步，`:34` 只exec corepack pnpm；根 `package.json:20` Rust -D warnings；CI `ci.yml:213` 使用期限。历史 `task-2-toolchain.txt:23` 只表示当时环境阻塞。 | 可复现帮助实现；Web `apps/web/package.json:9` oxlint warning政策仍属 G26 |
| `plan.md:77` Todo3：Weston 2×2探针、1280×800像素/IPC/清进程 | README:166有真实Weston启动，但其示例不是pixman/dimension像素验证器；没有找到本计划完整捕获驱动。 | GUI/IPC/清理验收债；未运行Weston，不能用现有命令称通过 |
| `plan.md:85` Todo4：named 8MiB/平台/C06依据 | 后续 ADR-0019:29 已授权512+1MiB，生产 `routes.rs:41`、`server/lib.rs:109`一致；C06由ADR-0023及diagnostics:62移出范围。 | 旧8MiB限制与C06等待数据已取代，不要求保持错误旧门槛 |
| `plan.md:93` Todo5：真实WASM/Server/Tauri parity matrix | Server `routes.rs:964`/`:999` 与Tauri `lib.rs:243`/`:257` 有host-parity feature入口；完整 `scripts/simulation/host-parity.mjs` 未找到。正常生产宿主仍见现有G01–G05/G18–G20。 | 功能入口有；完整真实驱动/跨宿主验收未证实，不重复G项 |
| `plan.md:101` Todo6：fresh/atomic checkpoints、digest/source、kill/resume、不完整禁止finalize | `baseline-run.mjs:61` checkpoint v4；`:683` 源指纹，`:756` 构建/二进制定位；`:1043` 逐seed receipt recovery、`:1054` 重复拒绝；`:1387`/`:1389` 未完成不发布完整manifest。`verify-simulation-artifacts.mjs:156` 资源、`:195`指纹、`:226`二进制、`:310`原始报告深验。 | 已实现工具主链；本轮未执行kill/resume或长矩阵 |
| `plan.md:109` Todo7：旧10×30交易日/5×400自然日矩阵与敏感性封存 | 后续 `.omo/plans/escrow-parallel-engine.md:269` 明确批准语义代表性primary5自然日64 retail、cross-year8自然日32 retail，保留10 seed、7 unique sensitivity与9 rerun；生产 `baseline-run.mjs:16`–`:25` 当前seed/倍率/时限，`:386` fixture profile，`:1391` manifest source。 | 新范围取代旧规模；不将有界fixture称完整市场压力，G39仍保留现行跨worker比较错误 |
| `plan.md:117` Todo8：default release全表面verifier | `scripts/check-web-release-wasm.mjs:15` 解析真实WebAssembly exports，`:26` 拒绝私有诊断export；Web package:8正式build接入；Server/Tauri/engine的feature/debug边界已有；Tauri `lib.rs:193` 禁用时明确unsupported。`release-contract.mjs` 未找到。 | 局部正式制品守卫已有；全表面真实HTTP/IPC/默认release完整验收仍欠，不能称只grep |
| `plan.md:125` Todo9：同步已验证边界/午休句/文档链接 | 正式diagnostics:62、testing:124、README:163已更新当前C06/CI/Wayland边界；`docs/causal-diagnostics.md:27` 仍保留「inherited decision clock omits lunch」，总账已明确午休时钟后续修复。 | 发现正式文档历史句滞后；不新增为生产时钟缺口。各文档更新不得伪造矩阵通过 |
| `plan.md:133` Todo10：verify-plan/final receipts/digests/all surfaces | `run-full-regression.mjs` 与K7 root verifier可核对各自inventory/source/artifact，`verify-plan.mjs` 未找到；现行手动CI `ci.yml:201`/`:227` 有全回归/E2E。 | 完整聚合封存验收仍债；不能把相邻局部门禁合成不存在的总通过 |
| `plan.md:141` F1–F4 与 `:152` commit策略、`:157` success criteria | 未声称F1–F4全APPROVE；本批没有运行actual Weston/Server/Web manual QA或Git操作。最新ADR-0028与 `docs/testing.md:124` 明确产品发布不调用CI/测试/lint/smoke。 | 不恢复发布依赖测试旧策略；独立手动开发CI/完整验收口径仍保留 |

## 候选与反证

| 新候选方向 | 当前反证/判断 | 处理 |
|---|---|---|
| archive§2a旧REJECT说明当前公司存档是unknown裸透传 | Web分行业parser和accounting parser已有，task29后续civil/disclosure复核也有APPROVE记录。 | 不新增；仍需当前测试运行才能宣称全验证 |
| Server民事日失败继续跑、恢复没新timeline、resync不发baseline | actor rollback/stop_with_failure、restore generation/public revision、WS baseline cursor gate当前均接入。 | 旧错误已修，不重复登记 |
| 不见完整parity/release/verify-plan脚本就是总账遗漏 | `docs/implementation-gaps.md:163` 与总账R20已明确记为验收债；各host feature/默认release守卫不等于该完整驱动。 | 已覆盖，保留验收债 |
| current baseline5/8天违反原400天需求 | 后续escrow Task11有明确代表性fixture修订与94次执行边界，工具不自称完整规模压力。 | 不复活旧矩阵；G39仍有独立现行问题 |
| `before` CLI/旧sealed corpus消失须补回 | `docs/test-cleanup-checklist.md:55`批准禁用before collector，`:86`批准移除sealed适配/装配，`:88`接受旧证据不可执行复验。 | 获批退役，不新增 |
| 8MiB放宽是静默绕门槛，C06仍欠真实数据 | ADR0019明确传输入口额度、ADR0023明确不使用真实市场数据；代码/正式diagnostics已一致。 | 取代/不支持，不新增 |
| 午休时钟仍漏实现，因为正式causal文档原句还在 | 文档历史句确未更新，但当前归档复核/总账已追后续时钟修正；该句不能反向证明当前生产时钟失败。 | 文档滞后记录，生产不新增G |
| strict Corepack/Clippy历史阻塞尚在 | helper给明确Node/Corepack诊断，原unnecessary_filter_map源码已替换；普通Web测试最新直接调用Node shard，发布不跑lint。 | 不重开旧环境问题；真实Web warning契约缺口仍G26 |

本批只完成静态全文/入口/批准范围复核，没有实际运行 GUI、IPC、Node测试、Cargo、baseline或发布。全部历史通过数/旧REJECT都只代表原时点；没有用其替代当前运行证据。
