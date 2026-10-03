# 隐藏扫描批次 083

- 基线：`43b1aa5`；source root：`/data1/baiyifan/workplace/stock_market_game`；caller：`/data1/baiyifan/workplace/stock_market_game/.worktree/implementation-reaudit`。
- 规范：已读 caller `AGENTS.md`、`docs/principles.md`、`docs/open-questions.md`、ADR-0010、ADR-0025 及现行 implementation audit 总账和宿主复核。历史 agent instructions 仅视为来源，不作为当前规范。
- 方法：三份 source 均连续读取至 EOF；实测行数和 SHA-256 匹配 scan-plan，aliases 均为 1，无重复别名或截断补读。只写本批记录；未运行测试、构建或 Git，也未改产品代码。

## 来源全文与判断

| 来源 | EOF / 行数 / SHA-256 / aliases | 章节状态与结论 |
|---|---|---|
| `agents/oop-refactor-audit/exhaustive/reviews/web-04-tauri-final.md` | EOF；35 行；`3aeb425d1536fd8818810504a0a80178f3790908c6fc3c76db4f41c522016bad`；1 | 完整读取：结论、大 A 语义、必要性与范围、独立发现、复核状态与限制。结论只核准 Tauri facade 对象归属盘点；listener 部分注册失败与 create 成功后的初始化失败是明示的独立资源清理风险，并未宣称修复或纳入 OOP 承诺。 |
| `agents/oop-refactor-audit/exhaustive/reviews/web-04-wasm-final.md` | EOF；29 行；`88972f9b55980a1a4eb64429eaeea4161e1b958631d13adbc35bb3fb81ec1767`；1 | 完整读取：结论、核实事实与判断、独立后续边界、限制。重复 create 覆盖旧 handle 被明确列为独立生命周期风险；该文限定为审计对象归属，不承诺改 Worker 行为。 |
| `agents/oop-refactor-audit/exhaustive/reviews/web-04-worker-final.md` | EOF；20 行；`9ede7fe9b4328d820faeed6097ea1b5c836c1395762e4cf4f9e24fe2d1a65ed7`；1 | 完整读取：复核范围、最终材料指纹、结论、复核发现、限制。load 晚到写回、`barrierPaused` 仅校验整数、`requestWorker` 同步 post 失败延迟清理均被明确记录为既有边界；复核没有声称这些行为已修复。 |

## Caller 现状、关联与候选

- 大 A 与必要性：来源审查的 Web/Tauri/WASM 生命周期及 OOP 归属，没有改变沪深交易制度、金额/数量单位或撮合语义；现行 ADR-0010、ADR-0025 无需据此扩张。未发现可以证明旧 OOP 核销错误的新增证据。
- Tauri 调用链：`apps/web/src/host/tauri-host.ts:113-130` 在初始化 `try` 前依次注册两个 listener；`:132-141` 的 catch 仅 unlisten 两者并抛错，未在 `create_session` 已成功后回滚 session。源 review 已把这两项标作独立初始化清理风险，明确不作为对象设计缺陷；总账已有 G51 针对 dispose 时 `stop_session` rejection，但不等同于此初始化路径。它们不是该 OOP 审查所批准的对象归属承诺，且没有被源文误报为已修复，故本批不新增候选。
- WASM 调用链：`apps/web/src/host/worker-host.ts:182-184` 收到 ready 后发 create；`apps/web/src/host/wasm-worker.ts:133-150` 消费每条 create 并调用 `slot.create`。历史复核 `agents/implementation-audit/exhaustive-review/luna61.md:54` 已说明覆盖旧 handle 的风险仍在，并限定常规用户路径未见同一 Worker 重复 create，不把它列为用户可达泄漏。无新的运行证据，不能把它升级为遗漏承诺。
- Worker 调用链：`worker-host.ts:353-364` 的 load 在 await restore 后直接安装 generation/baseline；`:226-228` 对 barrierPaused 仅做 generation 格式校验。`worker-request.ts:43-73` 只按响应 requestId/generation 相关联，同步 `postMessage` 抛错保留 timeout 清理时序。既有 `luna59.md:24` 已说明同步 throw 延迟清理和 dispose 时 pending 请求不立即拒绝是明确排除项；`luna61.md:53-54` 说明 restore 发布顺序及 Worker 重复 create 边界。上述行为不是对象抽取承诺，来源也明示限制，不能按静态可见性推断成新运行故障。
- 总账交叉检查：G04 及 `reaudit-host.md:16` 记录 Remote/Tauri 重送 baseline；G51 记录 Tauri dispose 的 stop_session 错误出口；`luna29.md:33-37` 记录 Worker restore `nextGeneration` 单调性另项。没有将本批所列 OOP 结果宣称能核销这些事项；没有发现来源中旧“通过”结论覆盖到其外。
- 候选：无。以上初始化、重复 create、load/stale 与请求清理项虽有静态代码路径，其中来源明确标为范围外/既有边界，且本任务只登记已批准承诺遗漏或旧核销错误；没有证据满足该门槛。未运行行为测试，故不对可达性、实际资源泄漏或运行时结果作结论。

## 结论

三源完整性核验通过。未发现已批准对象归属承诺遗漏或旧 OOP 核销错误；现有材料中的生命周期风险均未被旧审查谎称为已修复，且部分风险已在 caller 现行总账/宿主复核中独立登记。结论限于本批历史材料与当前静态代码调用链，不代表测试或运行时验证通过。
