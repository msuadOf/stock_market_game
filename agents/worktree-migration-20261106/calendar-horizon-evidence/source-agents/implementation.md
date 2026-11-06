# 新增公司 due 越过日历上界的来源修复

- 冻结边界：运行日期最大为 `2099-12-31`；`2100-01-01` 不是合法运行日期。见 `docs/simulation-calendar.md` §1–2。
- 原先行红：根任务登记的 `day_end_new_due_beyond_calendar_ceiling_rolls_back_every_domain` 在真实 2099-12-29 新增到期日 typed 拒绝后，会话被误标 poison，完整 SaveSlot rollback 断言无法完成；该证据没有证明 World 已 terminal。本轮不改、不重跑该 integration case。
- 本轮范围：仅以本次真实准备中的私有来源标记区分 due-horizon 拒绝与其他同型 CalendarError；测试覆盖 internal/external owner 真实新增 due 与同型 callback 负控各一。
- 验证状态：实现与四个新 case 已落盘，等待集成方运行与核验；本轮仅执行单文件 `rustfmt` 与 `git diff --check`，按父任务要求未运行 Cargo、rustc 或 case，不宣称编译或测试通过。
