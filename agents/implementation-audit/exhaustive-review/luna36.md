# luna36：NPC/company notepad 历史阻断与本次发布文档复核

## 范围与方法

- 基线：`.worktree/implementation-reaudit` 的 `08e4fc7`，其后 merge `a7c7ce3`；08e4fc7 是文档单文件变更。
- 按顺序读完 `decisions.md`（7 行）、`issues.md`（1373 行）、`learnings.md`（1474 行），共 2854 行；issues 与 learnings 均以 EOF 收尾，无截断续读遗漏。
- 逐段读取大文件（issues 每 150 行、learnings 每 150 行），并另检视全部章标题、历史 REJECT 后续段、已有审计总账与当前相关生产调用者。
- 必读规则：仓库 `AGENTS.md` 与 `docs/principles.md` 已读。只新增本报告；未写产品代码、未改 Git 状态、未跑长测试。

## 章节覆盖矩阵

| Notepad | 全文章节主题与追踪结论 |
|---|---|
| `decisions.md` | 仅有模板标题、自动脚手架说明及分隔线，无实际决策条目；不存在可遗漏的决定或后续调用者。 |
| `issues.md` 1–178 | Task 39 存档/传输尺寸；Task 35 诊断适配器及桌面验证；Task 30 WASM DTO/date；Task 32 IPC/系统依赖；Task 29 civil/disclosure events；Task 33 query/K7/RemoteHost；早期任务 1–7、21 的工具、政策取证、日历、会计与计划状态机。按后续实现记录核销的阻断见下表。 |
| `issues.md` 179–410 | 任务 21 可达性修复；CAS 官方取证二轮纠错；Task 4 历史乱码；Task 6–8 会计/流程/审查门禁；旧 HANDOFF 段严重乱码；Task 8/17 复核及 Task 19/9/20/11/10/12/14 的问题条目。乱码不据上下文补造内容。 |
| `issues.md` 411–900 | Task 19、9、20、11、10、12、14、13、15、16、25、18 的逐项问题；两次明确复核 REJECT（Task 10/12/14/13/25）及后续任务指针、登记门禁、会计/语义边界。Task 10/12/14/13/25 REJECT 的修复状态后续有记录；历史拒绝不应当作当前拒绝。 |
| `issues.md` 901–1200 | Task 15/16/25/18、Task 23/22/24 的设计和验证限制；Task 24 完成轮；Task 26 过渡存档边界、参数、执行与 V 删除、审查遗漏。Task 26 市场测试误删的修复在下一节继续记录。 |
| `issues.md` 1201–1373 | Task 26 REJECT 修复轮；Task 27–34/36/39/35 交接、修复、阻断和终验条目。Task 36 最初阻断后已有实现、clock 修复及 ACCEPT；Task 39 顶部 8 MiB 状态与当前代码/后续发布记录不一致，列为文档总账滞后。 |
| `learnings.md` 1–234 | Task 39/35/30/32/33、基线、多 seed、纯移动、官方法源取证经验；覆盖任务 1–3、5、6、7、17、8、19、9、20、11、10、12、14、13、15、16 的主要架构、领域契约及边界。Task 4 章节标题及正文乱码，明确记作不可判读。 |
| `learnings.md` 235–538 | Task 21 状态机与修复；Task 6 会计、Task 5 时钟、Task 7 公司实体、Task 17 profile、Task 8 工商会计。守卫与事件领域前提互斥的历史故障已由实际修复与测试记载。 |
| `learnings.md` 539–890 | Task 19 技术指标/个人记忆、Task 9 银行、Task 20 经历、Task 11 地产、Task 10 保险、Task 12 合并、Task 14 经营调度。含已登记简化、老 coverage gaps、领域 RNG 和现金流语义。 |
| `learnings.md` 891–1202 | Task 13 报表/结账、Task 15 披露、Task 16 获知、Task 25 注意力、Task 18 信念、Task 23 紧迫度/报价、Task 22 分配、Task 24 订单生命周期。历史 REVIEW REJECT 的修复与批准在各自学习条目或 issues 追加中继续核验。 |
| `learnings.md` 1203–1474 | Task 26 决策链与共同 V 删除；Task 27/28/29/30/31/32/33/34/36 后续修复、宿主、DTO、UI、诊断及时钟记录。Task 31 最初 blockers 与 Task 32 环境阻断均有后来解决记录；Task 36 的最后状态为独立修复 reviewer ACCEPT。 |

## 历史 REJECT 与阻断复核

| 旧条目 | 后续决定/当前代码核验 | 当前结论 |
|---|---|---|
| Task 10：保险游戏假设未进 policy/docs，另 rate 上界用例缺失（issues 492 附近） | 当前 `docs/company-accounting.md` §保险简化登记及相关测试/coverage 文档已纳入；原五项简化是登记门禁，不需改引擎语义。现有上限修复由历史 review 跟进报告说明。 | 已有后续修复证据；不重报 REJECT。 |
| Task 12：合并假设未登记；多条同成员对申报可复用首个镜像导致双倍抵销；failure 测试超行（issues 735–760） | 现 `docs/company-accounting.md` 登记 D1–D6，明确重复申报类型化拒绝。当前 `accounting/consolidation/eliminate.rs:36-55` 在构造工作表前检查成员对重复并返回 `DuplicateIntercompanyDeclaration`；`handled` 在 `:57-76` 避免重复消费。 | 关键静默错账已修并具类型错误路径；门禁已登记。 |
| Task 14：经营参数假设未登记、clippy；0 日期限配置可造成次日 `DueSkipped`；冲击事件行业不适用但可能激活（issues 773–790） | 当前运行参数登记见 `docs/company-accounting.md`；`operations/config.rs:156-186` 对所有行业日期参数统一拒绝 `<1`，并由 live/history 共用 builder 调用（既有审计记录已核对）。非适用冲击仍是 G36 现行候选，不因 F-O2 修复而核销。 | F1/F2/F-O2/F-O3/F-O4 各按修复证据处理；F-O1 仍挂总账 G36，非新发现。 |
| Task 13：更正损益在后续年度重计；附注 `unwrap_or(zero)` 吞溢出；报表假设漏登记（issues 855–890） | `accounting/closing/mod.rs:365-399` 每次生成都取得该 scope 累计 restatement 映射，传给 `generate_report_set`；现 `docs/company-accounting.md` 登记结账简化。旧 reviewer 的后续复验记为修复完成。`notes.rs` 已不再对减法溢出调用 `unwrap_or(zero)`（原代码位置须读当前实现确认，不把历史行号当现行代码）。 | 历史主要 defect 有修复记录；本文没有发现可复现的当前调用链反证。 |
| Task 25：attention 五参数未登记、审查主因 REJECT（issues 1024–1050） | 当前 `docs/company-accounting.md:158` 登记 `game-assumption-attention-discovery-weights`，包括版本化发现参数；任务 25 的复核补充记录明确确认最终提交树和逐项值一致、翻转 APPROVE。 | 已解决。 |
| Task 26：删除 `tests/market.rs` 连带删 A 股价格笼子测试；计划变更前未撤在途子单；pending 事件无清理规则（issues 1195–1240） | issues 修复轮记录从 ccf0490 恢复非 V 价格笼子语义测试并拆分测试文件；执行路径在重构后仍由 `plan_execution`/`decision_chain`/`auction_day_end` 处理 linked order 生命周期。pending 事件保留规则仍由 Task 27 明确收口；本次目标 commit 是 docs-only，不触交易代码。 | A 股价格笼子覆盖删除是历史已修复事件；pending 事件的后续策略应以 Task 27/当前存档契约为准，未见在本提交引入回归。 |
| Task 31：日结失败吞错、restore timeline、resync baseline、publisher revision 分段、WS 隐私（learnings 1311–1382） | 后续 Task 31 repair 明确写入失败阻断不能伪成功、恢复 timeline/revision、重发 baseline、header-only token、player-only baseline 及深度/尺寸限制。当前审计已有 Server host-caller 复核；08e4fc7 无 Server 文件变更。 | 历史 blockers 已由后续实现记录覆盖；不能沿用旧 REJECT。 |
| Task 32：无共享 civil/disclosure event/query API、GTK/WebKit 阻断（issues 58–69、learnings 1321–1338） | Task 29 后续完成 shared events；Task 32 resumed bridge 记录 shared event 与 query 已可用。Task 35 文档记录 Linux GUI prerequisites 最终安装并在 Xvfb 启动/MockRuntime 验证；本次 main Release 记录三平台构建，但不等于本轮独立重跑。 | 当前源/API blockers 已解决；环境限制是历史事实，不能宣传为本审计重跑的 GUI/IPC 验收。 |
| Task 34：WASM nullable/date producer mismatch、controlled date 显示 fallback（issues 1309–1338） | Task 30 中央 `PublicReportSummary::from` period-end 转换解决跨 host 月份/日期契约；Task 34 final browser + controlled-date review repair 证明 strict UI、不撒谎显示空日期。当前 re-audit 另注明无需以旧失败误报当前源。 | 已修复；过时 browser failure 不作为现行缺陷。 |
| Task 35：server/Tauri diagnostics adapters 未实现（issues 11–18） | 随后的同日期 Task 35 adapter completion evidence 列出 authenticated Server route、Tauri command及真实功能/无功能测试；GTK 最初阻断在后续 native verification resolved 条目处理。 | 已有 adapter completion 与环境修复记录，旧 incomplete 状态被后续条目覆盖。 |
| Task 36：事件缺权威成交关联/时钟阻断（issues 1339–1373） | 同一 issues 后续记载 source ledger、真实订单/成交关联、restore 边界和 clock 修复；最终 reviewer ACCEPT，明确旧 hash 可用反向秒数重现。 | 不再是未实现阻断。历史性能/feature 结果只代表当时报告。 |

## Current caller 与新总账观察

1. **notepad 的 8 MiB blocker 陈述过时。** `issues.md:7-9` 仍称服务端 `MAX_LOAD_BODY_BYTES` 未变 8 MiB、29/69/133 MB 档案一概不能远程加载；`docs/superpowers/specs/2026-09-13-company-information-issues.md:16` 和相关 learnings 副本仍保留同类句子。当前 engine `packages/engine/src/session/persistence.rs:943-956` 默认解码边界是 512 MiB；Server `apps/server/src/routes.rs:39-42,809-838` 把 body cap 设为 engine limit + 1 MiB，并在同一入口用 512 MiB `SaveDecodeLimits`。仓库总账 `docs/implementation-gaps.md:174-175` 已明确称旧 8 MiB 说法过时。故旧 Task 39 的“全部尺度超 8 MiB，远程不可用”推论不得沿用；早期大小仍只是旧样本。
2. **新的大档案边界已在本次发布记录显式披露，但没有进入这组历史总账。** `agents/main-release-validation/summary.md:23-27` 记载 100k 完整日最大序列化档案 591,344,527 bytes，超过现行 `MAX_SAVE_DECODE_BYTES=536,870,912`；`GameSession::restore`/typed JSON 压测通过并不能证明 Server 的有界 HTTP restore 可以接收它。该文档同时保留容量边界，不抬高限额。建议总账将 Task 39 状态更新成“旧 8 MiB blocker 已取代；100k 完整日样本越过现行 512 MiB Engine decode / 513 MiB HTTP body 上限”，并标成已记录的容量验收限制，不混成一般远程失败结论。当前证据是发布验收记录中的测量，不在本任务重跑。
3. **Task 14 F-O1 的 G36 追踪已有总账归属。** 历史 review 提醒经营冲击类型不按行业过滤，银行/保险可能记录无效激活；新审计文件 `implementation-audit-2026-10-02.md` G36 保留行业场景生产查询边界，`sweep24.md` 再次解释该问题。它是已知候选，不能误报为 notepad 新遗漏。
4. **Task 19 价格记忆 `prune` 的生产调用欠账已有候选编号。** learnings 任务 19 将 prune 保护集留给 25/26；现有总审查 `sweep21.md` / `luna21.md` 已登记 S21-C01，指出生产 roots 更新 memory 但未调用 prune、恢复边界不是 protected+8。不能把它作为此轮新发现或被 watchlist 修剪误核销。
5. **政策登记门禁与未取到的一手依据分开。** Task 10/12/14/25 的游戏假设登记现可在正式会计文档定位；地产 CAS 17 原文、个别税法/休市通知/法规原文缺口仍是法源 provenance 或简化披露问题。不能从旧 fixture `blocked` 状态推断功能没做，也不能声称已取得官方条款。当前 A 股语义改动为零（08e4fc7 仅调整验收文案），无新增交易规则主张。

## 08e4fc7 diff 审查与结论

- 完整 diff 只有 `agents/main-release-validation/summary.md` 6 行改写/新增；无 engine、Web、Server、Tauri、fixture、交易规则或存档 API 文件变更。
- 将旧 8 MiB 验收 blocker 改成真实 512 MiB decode gate 的报告、上传后校验 tag SHA、最终 Release/Pages/workflow/manifest 状态，以及“不等价公网完整游戏验收”等限定，和目标树代码/已有工作记录的口径一致。
- A 股交易含义、单位、撮合、T+1、价格笼子、结算与存档契约没有变化；无需借历史日历/行业会计规则扩大该 docs-only diff。
- 独立扫描没有发现 08e4fc7 的文案把旧 REJECT 当成当前失败，亦没有发现其声称的产品实现变更。主要总账维护项是上面的 8 MiB 历史副本需要 supersede，且 591 MB 负载边界虽然已在 release summary 中透明记录，宜被核心实现审计总账交叉引用。
- 旧记录的乱码章节仅标识为“不可判读”；没有按拼音、残留数字或邻接段猜补决策内容。
