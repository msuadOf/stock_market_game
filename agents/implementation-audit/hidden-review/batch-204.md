# 隐藏审计批次 204

## 范围与完整性

- 计划基线：`43b1aa5f25226c72976ca172d32f4a8eeb2272ad`；当前 caller `HEAD` 与基线一致。三个源文件均在工作树中，但都不在该基线提交内；因此本批只能确认当前工作树来源内容，不能将其描述为基线已跟踪材料。
- 三份源文件均已完整读至 EOF；当前 SHA-256 和行数均与 `scan-plan.json` 相符。

| 源文件 | 行数 | SHA-256 | EOF / 基线状态 |
|---|---:|---|---|
| `agents/oop-refactor-audit/chinese-localization/before/exhaustive/reviews/engine-tests-07.md` | 86 | `db77997e8b3999a12261b4c7b534e67670c0be938dce4d40a1b3c14df10e7110` | 已读至 EOF；不在基线提交 |
| `agents/oop-refactor-audit/chinese-localization/before/exhaustive/reviews/engine-tests-08.md` | 86 | `f8abff0fe606c7f07eb688b198d6795e61896c32bff5ce0fe5fec914a060b6a6` | 已读至 EOF；不在基线提交 |
| `agents/oop-refactor-audit/chinese-localization/before/exhaustive/reviews/engine-tests-09.md` | 36 | `2ffef988d7d7a9a8599d3e0d172974f2170d408d6a4e71dda88276685d0c9b6e` | 已读至 EOF；不在基线提交 |

## 当前责任与调用消费

- `engine-tests-07.md` 是历史审查记录，映射到计划分配、计划、协议及披露集成测试。当前 `packages/engine/tests/plan_allocation/main.rs` 显式挂载 `failures`、`gold`；披露及协议测试通过对应 `packages/engine/tests/publications/` 与协议测试文件承载。它们通过 engine API 测试，不是生产代码 owner。
- `engine-tests-08.md` 是地产会计、存档契约和恢复规模测试的历史审查记录。当前 `packages/engine/tests/real_estate_accounting/main.rs` 挂载 `failures`、`gold`；`packages/engine/tests/save_contract/main.rs` 挂载 `failures`；`packages/engine/tests/scale_restore_limits.rs` 是独立集成测试源。记录中的地产资本化阈值明确属于游戏假设；其来源限制不构成现实会计规则依据。
- `engine-tests-09.md` 对应当前 `packages/engine/tests/session.rs`。它是 Cargo 集成测试源，通过 `GameSession`、`SaveSlot` 等公共 engine API 验证；无法从这份历史审查记录推断其生产实现 owner。该文档准确区分非法存档拒绝和既有运行会话失败原子性。
- `packages/engine/Cargo.toml` 只显式声明带 feature 的诊断集成测试，以上普通集成测试按 Cargo 的 `tests/` 约定自动发现。这里确认的是当前路径、入口及消费方式，不是重新验证所有条目所述的测试行为。

## 发现与限制

1. **基线归属限制（确认）。** 三个历史 review 文件都无法由 `git show 43b1aa5:<path>` 从指定基线读取。计划 hash/line 绑定到的是当前工作树副本；本批不把它们误报为基线源文件，也不推断其提交来源。
2. **历史状态不能视为当前独立复核。** 各文件记录了修订前问题、修订后结论及之后的文档绑定。其中 07 记录 allocation 测试数从 12 改为 11；08 记录 fixture 说明曾错配后校正；09 记录“恢复失败原子性”措辞已改成存档拒绝/输入校验边界。这些仅为所读历史文本所述，未在本批重新打开相应 item JSON、复读全部生产/测试源码或执行测试。
3. **大 A 语义边界。** 这些文件审核的是 engine 测试和审计材料，本批没有新增或实现交易规则，也没有查阅官方现行规则。本批仅能指出历史材料对沪深、T+1、整手等测试语境的描述及其明确限制；不对法源真实性或当前实现的规则正确性作通过声明。

未运行测试、构建或 Git 写操作；未修改产品源码。批次状态：**complete（限于来源指纹、历史记录阅读及当前路径/入口核对）**。
