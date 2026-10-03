# 隐藏扫描批次 032

## 范围与全文读取凭据

- 按 `.worktree/implementation-reaudit/agents/implementation-audit/hidden-review/scan-plan.json` 的 batch 32、owner 2 来源清单，逐一连续读取至 EOF；工作树基线 HEAD 为 `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`。已读主仓 `AGENTS.md`、`docs/principles.md`。
- 三个来源实测行数与 SHA-256 均与 plan 一致：

| 来源 | 行数 / SHA-256 | EOF 与完整章节矩阵 |
|---|---|---|
| `agents/oop-refactor-audit/exhaustive/areas/engine-foundation.md` | 45 / `c837cf61bc5ee1a17054a0dca271c928ef1ea51a46483bdbe830b49757f93ed8` | 全文：标题及候选未实施声明、4组材料链接、跨文件对象归属表（Account/Position、GameConfig、日历、Market、OrderBook、Money、行为决策、经历、诊断、观察、evidence、timing、compute）、6项去重候选与实施顺序、验收边界。最后一节说明文件核销/未实施状态；到 EOF。 |
| `agents/oop-refactor-audit/exhaustive/areas/engine-tests.md` | 35 / `9cb816412b741e61838cce4a22cfd2eb9f3f4fc37283f9358decde5783b1ee55` | 全文：候选未实施声明、01–10批测试映射表、6步跨层迁移验证顺序、独立复核状态与限制、最终单文件核对及候选分类。明确测试文件只验证现有对象，不把测试 helper 转为业务对象，且本审计未运行测试；到 EOF。 |
| `agents/oop-refactor-audit/exhaustive/areas/hosts.md` | 51 / `4dc8f3492995ce40db24c70d4e085a96ad2f93fb1021747ee0db4d5963d274c1` | 全文：候选未实施声明、桌面/Server/WASM owner 表、跨文件调用契约、5项迁移建议、三批覆盖/复核状态、最终单文件核对与候选分类。明确历史审计命令误用、WASM handle 回绕、step_frame/tick_batch 原子性未证等限制；到 EOF。 |

上述行数、哈希、EOF 状态也记录在同目录 `batch-032.json`。全文历史任务建议仅视为审计材料；来源自身标记的候选设计/未实施不等于已批准承诺。

## 当前 caller、缺口关联与反证

- **Account / Position**：现基线仍有 `packages/engine/src/account.rs:95` 的 `Account`、`:609` 的 `Position`；来源提议私有字段/受控写入口，但也明确禁止新增任意生产改账能力。当前总账未把其结构提议列为已批准缺口。此为 owner/封装候选，不构成业务规则缺失。
- **日历**：当前 `packages/engine/src/calendar/policy/validation.rs:79` 计算 policy digest，`:125` 校验 `source_digest` 非空；现总账 G15（`agents/implementation-audit/implementation-audit-2026-10-02.md:66`）关注官方年度覆盖覆盖模拟回退，与来源提示的 digest 未编码 source_digest 是不同事项。digest 风险为来源已记录的待定义/验证项，本批不重复登记新 G。未据此重新判定官方日历内容或法源正确性。
- **CivilInstant / serde**：`packages/engine/src/calendar/date.rs:251-256` 定义值类型与构造入口。来源指出派生反序列化可能绕过秒域检查；它是边界风险提示，不代表已证明生产存档路径可以构造非法值或出现可达交易错误。本次未沿全部 serde 消费链证明可达性，作为未核实风险保留。
- **经历写入失败**：现基线 `packages/engine/src/experience.rs:192-244` 的 legacy `record_fill`/`record_fill_with_order` 进入转换逻辑；`experience/position_transition.rs:56-` 处理更新。来源明确部分 checked 溢出可能失败前部分修改，dated feedback 尚未提交。主总账 G08（主台账第 51 行）跟踪 legacy writer 未接日期/衰减消费链；不是同一问题。局部失败原子性为来源指出的额外候选，但其合法生产输入可达性、现有契约要求与最小修复方式本次均未证，故不提升为已确认缺口。
- **Market / OrderBook**：来源说明 `Market` 组合簿、`OrderBook` 管索引与顺序，且失败路径可能留下部分簿/索引变更；这不支持“失败会 rollback”的承诺。来源未提出已批准的行为变更。engine-tests 的 10 组迁移矩阵是测试导航，不能把缺本轮执行测试当成产品缺陷或通过证明。
- **宿主**：当前 WASM `apps/web-wasm/src/lib.rs:43` thread-local registry；`:60` insert、`:74-89` 查找/删除/step_update，导出 step 路径为 `:349-350`，统一访问 helper 为 `:519-526`。来源中 SessionRegistry 只是候选命名/聚合，不要求增加锁、异步化或改变 worker 单写者。handle `u32` 耗尽回绕被来源末节列为独立风险，但没有普通旅程可达证据；现总账 Q20（主台账 `:158`）已有登记，不重复建项。
- **计时/evidence**：`packages/engine/src/verification_evidence/phase_timing.rs:334-405` 有 timing collector 与 Rayon registry 容量采样。来源明确 `runnable_threads` 不是 OS runnable-thread 计数，panic unwind 清理未覆盖；这是测量/清理边界，不应解读为交易处理缺失。来源所列跨 worker 验收与性能矩阵属于未执行验证，当前总账 G39、G61–G63 分别覆盖验证门禁/工具期限，不由本次全文读取核销。
- **现行已登记关联**：总账 G15（官方日历覆盖）、G16（tick 历史复制）、G17（批量生产计算接线）、G18/G19（宿主背压/倍率发布）、G39（K7真实受理轨迹）以及 G40/G53/G66（宿主确认、generation、远程已接受请求出口）各自有独立定义；本批来源提供 owner 与测试定位背景，没有反证其已解决。G15/G16/G17/G18/G19/G39之外的宿主关联非本批三篇所述运行时修复承诺，不据此推新编号。
- **无新增候选**：sources 的 A02 费用 helper 被明示为可选非 OOP 组织整理，提取不会自动改善原子性；纯函数/已有 owner 不应为类而类。engine-tests 的测试/fixure 支撑均无 OOP 操作。hosts 的路由拆模块、WASM registry 命名及现有 actor 结构均为候选；未发现已批准但被遗漏的产品实现承诺。当前无新增 G/Q。

## 结论与限制

三篇完整材料的主旨是结构所有权和验证导航，不授权改变撮合顺序、T+1、股/分单位、涨跌幅边界、日终保存等产品契约。来源已指出的 digest、反序列化、经历失败原子性、OrderBook失败部分变更、WASM句柄耗尽及计时panic清理仍应视各自可达性和契约独立处理；本批没有把它们误报为“未抽取 OOP 对象”的缺口。没有新增 G/Q，也没有发现可据本文核销的现行缺口。

未运行测试、构建或 Git；未核实 A 股官方规则法源（本批不提出交易制度结论）。
