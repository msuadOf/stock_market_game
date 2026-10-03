# Batch 224

基线 `43b1aa5`（当前 HEAD）。本批三份文件均为旧的中文本地化穷尽审查材料中的历史记录，不能作为当前生产源码证据；其内部结论仅对所述历史候选 delta 有效。

## 文件核验

- `web-07-runtime-final.md`：已全文读至 EOF，SHA-256 与计划一致，21 行。记载 runtime-v2 wire DTO 字段、parser 支持常量的历史 delta 独立审查通过；明确未执行测试，且不保证费用单位文档或运行时调用方事实。
- `web-07-snapshot-final.md`：已全文读至 EOF，SHA-256 与计划一致，21 行。记录 Redux snapshot writer、ProtocolCoordinator 与 chart callback 的历史 delta；描述 `installProtocolSnapshotBaseline` 与 `setSnapshot` 的双写顺序，以及 delta 分支，未建议合并或移除 dispatch。
- `web-07.md`：已全文读至 EOF，SHA-256 与计划一致，16 行。历史审查入口汇总若干 delta 通过状态；其自身声明不构成新的源码审查，实施仍需 TDD 与源码 diff 独立复核。

## 基线上的现行归属与调用

- `apps/web/src/save/schema/runtime-v2.ts` 当前拥有 `SaveRuntimeV2` wire DTO、封闭词表和 `parseSaveRuntimeV2`；`apps/web/src/save/schema/root.ts` 将其作为 `runtime_v2` 成员调用并返回解析结果。旧记录对纯解析器和字段归属的描述与当前源码相符，但当前文件检查不等于核验完整存档 writer/reader 消费面。
- `apps/web/src/store/store.ts` 当前定义并导出 `setSnapshot`、`installProtocolSnapshotBaseline`、`applyProtocolRuntimeDelta`、`applyProtocolFrame`。前两者的差异仍存在：baseline 同时改写 generation 并清空委托投影/readiness，普通 `setSnapshot` 只更新 snapshot 和 `lastSeq`。
- `apps/web/src/host/protocol-coordinator.ts` 当前拥有协议更新发布：安装 baseline 时 dispatch baseline action；civil-update 或带 runtime snapshot 的更新也 dispatch baseline；runtime delta 分支 dispatch `applyProtocolRuntimeDelta`。随后调用 baseline/applied callbacks。
- `apps/web/src/app/useMarketChartRuntime.ts` 当前消费 callbacks：`installBaseline` 与 `replaceSnapshot` dispatch `setSnapshot`；`acceptReduction` 对 snapshot 路径调用 `replaceSnapshot`，对无 runtime snapshot 的 frame 路径 dispatch `applyProtocolFrame`。因此旧 snapshot-final 中所述 coordinator baseline 后 chart callback 再 `setSnapshot` 的顺序在当前代码中仍可见。
- 消费边还包括 `apps/web/src/host/protocol-runtime-store.test.ts` 的测试回调（直接 `setSnapshot`）、`apps/web/src/store/snapshot-runtime-delta.test.ts` 与 `snapshot-local-refresh.test.ts` 对 store action/reducer 的断言，以及 `apps/web/src/types/generated/SaveSlot.ts` 对 `runtime_v2` 的类型引用。此处为静态读取与符号搜索，未运行测试。

## 范围与候选

没有发现这三份历史记录本身提出需在本轮实施的产品改动。若实现任务引用其 snapshot/runtime 结论，应以基线当前源码重新验证，并保留 generation、委托投影/readiness、seq 更新语义及存档 wire 格式。涉及 A 股交易制度的实质规则变更不在本批材料范围内；这些记录不提供官方规则依据，也不替代现行交易规则复核。
