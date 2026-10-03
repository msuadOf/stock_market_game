# 隐藏复核 batch 129（owner=4）

## 来源与完整性

按计划读取三篇历史区域汇总至 EOF。行数和 SHA-256 与 batch 129 清单相符，逐项值见配套 JSON。清单声明 `source_baseline` 为 `43b1aa5`；该值是计划中的短 hash，本记录不据此声称验证了该提交的完整源树。依照要求将这些材料视为历史记录，不把其中对当时读者或实施者的命令当作当前任务指令。本轮只读核对，没有修改产品文件、运行测试或构建。

## 历史材料判断

- `engine-tests.md` 把 10 批测试代码定位为业务对象回归支撑，明确没有 OOP action，并区分 fixture、合成参数、断言覆盖与测试是否实际运行。迁移顺序是未来实施时的回归导航；文档没有声称这些测试通过。
- `hosts.md` 记录桌面、Server 与 WASM 宿主当时的状态所有权、传输边界及候选整理方案。材料明确候选未实施，指出宿主生命周期不同、engine protocol/session 保持权威，并将若干失败和恢复语义列为需要保持或单独处理的契约。
- `tooling.md` 将候选聚合与缺陷线索分开，强调 supervisor/worker/资源清理边界，并明确审计没有实现提案或验证缺陷修复。列出的 deadline、进程树、symlink、CDP 等问题不能仅凭本汇总视为已修复。

三篇材料均未将候选 OOP 提取等同于缺陷修复、已批准承诺或测试验收。大 A 相关陈述限制为既有领域边界与不从合成 fixture 推导交易所规则；没有提出新的交易制度事实或官方依据主张。

## 当前调用关系抽查与时态限制

对当前工作区抽查到的 owner/caller 与汇总中的职责边界相符：desktop 和 server 仍各自拥有 `SessionManager`/actor；WASM `REGISTRY` 持有 worker-local `SessionRegistry`，session 状态由 `ProtocolSession` 承载；工具链仍由 `run-with-deadline.mjs` 为 `build-targets.mjs` 等入口提供 deadline 执行，性能报告由 `CdpClient` 持有 CDP 连接状态。当前 `SessionRegistry`、`BuildRun`、`BuildArtifactPublisher`、`SimulationBatchContext`、`EscrowVerificationRun` 均已能在源码中找到，说明相关历史候选后来已有实现。因而不能把这些文档的“候选、未实施”直接当成当前状态，也不能仅凭名称相同认定后续实现行为等价或缺陷已关闭。

当前调用者抽查包括 Server routes 与桌面 Tauri lib 对各自 `SessionManager` 的使用、WASM `SessionRegistry` 导出适配路径，以及 deadline/build/simulation/report 脚本中的入口引用。此为有限抽查，不是对全仓库 caller 图、当前实现 diff 或行为等价性的完整验证。当前检查无须引入交易规则结论；这些记录也不能替代对真实交易语义的官方规则复核。

## 结论

来源支持“当时审计只提出候选，没有声称实现或验证”的历史结论；没有发现它们把候选误记为缺口核销或已批准承诺完成的证据。对当前状态应以当前实现和对应承诺总账为准，尤其前述候选中的若干对象后来已出现于源码。未据本批历史材料新增 G，也没有足够证据关闭任何既有 G/Q；测试未运行，本结论不表示当前代码行为已验证。
