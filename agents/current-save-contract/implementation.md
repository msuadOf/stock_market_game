# 严格当前存档契约

本轮依据用户纠正处理：命名重构不是新存档代际，禁止 schema v3 和版本兼容。
基线为 main 的 `b5b8464`；本轮使用 `codex/current-save-contract`，补缺分支尚未合并。

## 当前结构

移除 `SaveSlot.schema_version`、导出常量与版本 header 分派；保留 `runtime_state:
SavedRuntimeState`、`simulation_policy_id = a-share-simulation` 和原有完整结构/业务校验。
Rust `deny_unknown_fields` 与 Web 精确键集明确拒绝任何额外版本字段及旧 runtime 字段，
没有 alias、旧档转换、双字段恢复或默认补齐。日终保存/启动与显式换档加载边界不变。
真实会计报告 revision、工具证据版本及密封历史原字节不属于本次存档版本标记。

## 测试与表示锚

先写无版本字段可解析、任意版本标记拒绝的测试。Web 行为红测明确失败于缺少
schema_version 的整数校验；实现后存档结构、存储及文件目标 48 个短 case 通过。
首次从根目录运行文件目标套件错误使用 Vite 工作目录，结果不能算业务红测；
切换到 apps/web 后复测通过。Rust 已运行当前结构解码、额外标记拒绝和旧 runtime
字段拒绝的 5 个代表性短 case。普通 Node/Rust 进程均经 10000ms 外部 deadline，
Node case timeout 为 10000ms；Rust 构建为独立长验证，jobs=32，300000ms deadline。

固定存档表示锚不从失败输出猜新值。独立输入为此前实际 producer 原字节
`.tmp/naming-refactor-implementation/digest-evidence/current-captures/`，先复现旧固定锚，
再仅删除存档根节点的版本字段原文，其他原字节保持；逐 tick projection 另校验
JSON 业务结构相等。此过程仅用于历史证据，不是产品转换或加载入口。

| 场景 | 输入 FNV | 移除根标记后的 FNV |
|---|---|---|
| replay mid | 14482572867267443968 | 11144175475449247039 |
| replay end | 13430331173908825780 | 11114411633457169759 |
| step auction 0 | 4551912575944664394 | 16816661624066813714 |
| step auction 3 | 8964527389656814803 | 17087109400303676999 |
| step auction 6 | 16877315163896328895 | 2354296198442943349 |

replay mid 来源 SHA-256：`0a7c8a7a3490dc6501be1a9bed4f6a5c3fe04f9021a6016220d57822adbf23b7`；
replay end 来源 SHA-256：`d2891b61b005642d7716bf79c1789b3f771e36f674961b023dfa94aaea267c7c`。
事件固定锚保留；交易、守恒、restore、种子扰动及 tick 顺序扰动断言全部保留。
当前生产者已实际匹配上述独立预期：受控 replay 固定锚用例通过，step_skeleton
6 个短用例通过，包括三个新锚及真实价格、日界、种子/tick 扰动边界。save_contract
31 个短用例、session 的完整必填字段用例及 runtime 的两个短用例通过。
总计 42 个不同 Rust case（含实际 SaveSlot 生成类型导出）与 48 个 Web case 通过；每条测试命令外部 deadline
10000ms，四个独立 Rust 测试进程并行，其中 save_contract/step 的 test-threads=8。
生成类型已由 Rust 的 export_bindings_saveslot 用例重新导出，fmt 与 diff whitespace
检查通过。Web TypeScript 编译通过；首次调用 pnpm 因未在进程 PATH 中而未启动，改为实际
Node 执行项目现有 TypeScript CLI 后通过，不计作业务测试失败。

非作者 `save_contract_review` 已审完整最终 diff，并独立复算全部旧/新摘要及两个
SHA，确认唯一变化为根标记删除；发现的 tech-stack 现行规范漏项已修复并复核。
其结论为 A 股语义未变、改动必要且无剩余有效阻断发现。该 reviewer 未运行生产者
单测，不把其独立摘要核对冒称完整回归。本记录不宣称所有补缺、未来功能或完整回归已完成。
