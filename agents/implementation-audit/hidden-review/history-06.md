# 历史来源补扫 15–17

基线：来源计划 `source_baseline=43b1aa5`。按计划数组下标 15、16、17，分别从 `c139a69d2a09220e349912376d4be83d8859795f`、`bb16fc8d4df101d86912baf9a809a5320df61db7`、`bb16fc8d4df101d86912baf9a809a5320df61db7` 读取；三篇均连续读至 EOF，没有截断。完整指纹见同目录 `history-06.json`。这些历史证据描述旧构建路线与一次 Wayland 验收，不自动构成现行需求。

## 来源 15：`scripts/desktop/README.md`

全文说明 Desktop 打包矩阵的 dry-run 与真实构建边界、原生平台 bundle、有限 Linux/macOS 到 Windows 的 NSIS 实验路线，以及各平台签名/安装工具责任。当前 ADR-0027 已接受原生平台优先并明确不扩大既有实验性交叉构建；ADR-0028 又将发行工作流限定为构建、打包、核验与部署。现行 `scripts/desktop/build-matrix.mjs` 的 `planCell`/`route` 和 preflight 实际消费矩阵策略。因此这份文档与当前职责相符；旧路线细节受 ADR 收窄约束，没有遗留产品缺口。

## 来源 16：Wayland current immutable evidence

全文是一次 Task 3 验收证据索引，列出 Wayland 环境、渲染截图、诊断测试、IPC 与清理回执，并明确指出截图只证明渲染基础设施；空白图表/裁切片段仍是范围外观察，headless Weston 没有键盘 seat，不能声称原生输入覆盖。它是历史验收范围的陈述，不是产品实现承诺。ADR-0027/0028 的现行产品决策不要求把 Wayland GUI、图表完成或键盘覆盖纳入发布链路；总账裁定表也将 Wayland/GUI 记录归为验收或来源边界。不能从证据缺项推出运行时缺陷。

## 来源 17：Wayland证据完整性复核

全文给出固定 run ID、证据文件 SHA-256 清单及复核命令，并说明 review 与 machine manifest 排除在自身 hash set 外以避免循环哈希。它证明的是该历史目录证据的完整性校验约定，不代表当前 CI 或发行工作流必须重跑该 GUI 验收。ADR-0028 明确发布链路不执行原生服务 smoke；没有证据显示该审计器是现行生产消费者或是发行阻塞项。

## 按当前决策、总账与消费者复核

- **已实现**：Desktop 诊断能力由 Tauri `host_capabilities` 与 `npc_decision_diagnostics` 命令提供，Web `tauri-host` 消费能力并发起查询；桌面构建规划由现行 build-matrix planner 与 preflight 消费。这里只确认这些相关契约的调用/消费存在，不把历史 Wayland 报告等同于本轮复测。
- **已取代/收窄**：README 所记交叉构建只保留 ADR-0027 明示的有限实验路径；不将旧文提升为新的跨平台构建承诺。发布范围按 ADR-0028，不要求运行 GUI smoke。
- **未来或范围外**：图表空白/裁切观察、原生键盘输入覆盖及更广泛 GUI 验收没有在现行 ADR 中成为承诺；不能由这三份历史证据创建新需求。
- **G/Q 对照**：总账中的 G 与 Q 裁定没有对应的构建矩阵或 Wayland 完整性缺口；总账明确把 Wayland/三平台 GUI/缺原始日志归为验收或来源边界。无需并入现有 G/Q。
- **新候选**：未发现具备现行需求依据和当前 caller 证据的新实现候选；没有新增编号。

本次只做历史文档与当前决策/消费者的静态核对；未改游戏代码、未运行测试，也未执行 Git 写操作。上述工具链与桌面验收内容不改变 A 股交易语义。
