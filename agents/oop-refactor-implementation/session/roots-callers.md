# decision_chain 的 ParentOrderPlan caller 短修

- 范围：仅修改 `packages/engine/src/session/decision_chain/quote.rs` 与 `lifecycle.rs`；记录写入本文件。
- 已阅读 `AGENTS.md`、`docs/principles.md` 与两个 caller 源文件全文，并核对 `ParentOrderPlan` getter 的实现。
- 失败依据：`validation/engine-build-02-errors.json` 在两个文件共有 10 个 E0616；`quote.rs` 5 个，`lifecycle.rs` 5 个，均为私有字段读取。
- 迁移：仅将 `parent.linked_plan_id`、`parent.active_child_order_id`、`parent.active_child_remaining_qty`、`parent.limit_price`、`parent.child_qty` 对应读取改为同名 getter。
- 语义：getter 直接返回原 Copy 字段；未修改 DTO 字段、表达式组合、交易判断、金额与股数单位、撤单条件或行动顺序。没有新增或调整 A 股交易制度，沿用既有规则依据。
- 校验：两文件定向 `rustfmt --edition 2021 --config skip_children=true` 成功；定向 `git diff --check` 通过；搜索未发现这些 `parent` 字段的剩余直接读取。
- 依照上级分工，未运行 cargo 或产品 tests，未新增镜像 tests，未执行 Git 写操作；统一编译验证与完整 diff 独立复核由上级协调，不能据本短修校验宣称整批验收完成。
