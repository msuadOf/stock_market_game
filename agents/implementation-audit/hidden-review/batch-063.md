# 隐藏复核批次 063

## 范围与来源完整性

调用方工作树：`/data1/baiyifan/workplace/stock_market_game/.worktree/implementation-reaudit`，HEAD `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`。来源根目录：`/data1/baiyifan/workplace/stock_market_game`；扫描计划基线：`43b1aa5`。

三份指定来源均从来源根目录连续读取至 EOF。行数与 SHA-256 均与扫描计划一致，每份均有一个别名：

| 来源 | EOF / 行数 | SHA-256 | 使用范围 |
|---|---:|---|---|
| `agents/oop-refactor-audit/exhaustive/reviews/engine-pipeline-unchanged-binding.md` | EOF / 26 | `f4201b9ba2bf4a5c42f50cdfa97dcf1620aa09caa904ef2e0790695e98733be3` | 仅为绑定摘要；明确不是新的源码复核，其既往复核限制不能证明当前行为。 |
| `agents/oop-refactor-audit/exhaustive/reviews/engine-session-01-manager-delta.md` | EOF / 35 | `d884b32d96f251a427dbc4ef979dd5c21b5b81fb0fc4584e8fa603a76f8453ca` | A01 私有状态候选的复核，以及必须保留的克隆/提交所有权边界。 |
| `agents/oop-refactor-audit/exhaustive/reviews/engine-session-01.md` | EOF / 13 | `2268b7439bec8e3e01ec3df386ecc1e7432ff4af31c70c114cbdf69a4402e218` | 指向先前审计记录的入口；说明先前完整复核是历史记录，并说明当时 A01 仍是未实施提案。 |

已读取调用方 `AGENTS.md`、`docs/principles.md`、`docs/open-questions.md`、ADR-0017 和 ADR-0018、当前 implementation audit 的 G 总账及 `agents/implementation-audit/reaudit-engine.md`。ADR-0017 确定了交易候选隔离和单点提交；ADR-0018 将 shadow copying 记为长期历史数据成本，并区分已接受的局部性能目标与更广泛的提案。开放问题未授权改变这些契约。

## 当前实现调用链

- `packages/engine/src/session.rs:1125-1135`：`GameSession` 在 facade 上保留 poison 与仅供测试的失败钩子，并只持有一个 `CommittableSessionState` 字段。
- `packages/engine/src/session.rs:1328-1340`：`clone_for_tick_shadow` 重置 facade 失败元数据，仅克隆可提交状态；提交时也只将候选状态交给 `commit_from`。
- `packages/engine/src/session.rs:4074-4166`：`clone_for_shadow` 明确克隆每项状态成员，包括 `plans`、`closing` 和 `library`；`AccountBook::clone_for_shadow` 的错误仍映射为 `StepFatal::InvariantViolation`。
- `packages/engine/src/session.rs:4169-4240`：`commit_from` 解构并安装状态，同时保留 accounts、NPC attention、retail experience 的专用并行 drop 替换调用。
- `packages/engine/src/session/pipeline/shadow.rs:10-22` 与 `packages/engine/src/session/pipeline/candidate_commit.rs:45-100`：pipeline 从权威状态创建候选，并在检查之后、候选提交边界处安装它。
- Root-read 路径不同：`packages/engine/src/session/decision_chain/roots.rs` 为 `RootReadContext` 克隆 `session.state.plans`；`agents/implementation-audit/reaudit-engine.md` 将此项及历史 shadow 克隆成本列为 G16。捕获后的 `Arc` 扇出不会消除初始 `PlanBook` 克隆。

## 发现与处置

A01 候选的核心边界已经实现：不存在并行的旧字段集合或第二个可提交权威状态，facade poison/hooks 也不进入候选提交；状态集合保持私有。上述证据仅支持该历史提案的有限结构性结论，不重新验证继承的大 A 规则、不复现先前完整源码复核，也不能证明未运行的测试覆盖充分。

来源和抽查的调用链没有显示当前交易语义漂移或新的功能缺陷。变更涉及状态所有权与候选生命周期；ADR-0017 的候选隔离/单点提交边界仍然成立。没有交易所特定规则变化，因此不提出新的官方规则主张。

G16 仍开放，A01 不会将其核销：完整 `PlanBook` root capture 仍存在，候选状态克隆仍会深克隆 `plans`、`closing` 和 `library`（`session.rs:4151-4155`）。这与当前 G16 总账记录的长期历史复制问题一致；这是单独跟踪的性能/所有权缺口，并非 A01 未完成的证据。本批不据此提出独立 G/Q 条目或新的产品要求。未运行测试、构建或回归，未执行 Git 写操作。
