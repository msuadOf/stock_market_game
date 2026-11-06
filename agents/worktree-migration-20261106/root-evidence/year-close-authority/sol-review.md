# 年末权威链目录身份修复独立复核

2026-10-25；审查者：未参与实施的 `/root/sol61_proc_exit_review`。

**APPROVED：仅批准 `packages/engine/tests/company_scenarios/lifecycle.rs` 的 CompanyId 来源纠正。**

- 补丁：year-close-identity.patch（历史路径：`../../.tmp/year-close-authority/year-close-identity.patch`），SHA-256 `c4dfc783af6865416fddec903409af80f845b7ef82f00fe0751a7c94af48b503`。
- 基线文件 SHA-256：`da854dd22aba9b4ecbbd9a8d30a5c40ce579bffedcc8deb245daafea71d879d6`。
- 批准文件 SHA-256：`212c050a393303964a6a3b50475261c7e98ede4927a40ddcae043c544fc22109`。

审查者已逐段核完整 diff、真实探针、原红与新绿，重算实际新旧 binary 和只读 engine rlib SHA，核对 31 份原件指纹及当前 1679 项绑定源码；独立 `git apply --check` 退出 0。未实施、编译或运行测试。核对记录见 sol-review-binding.json（历史路径：`sol-review-binding.json`）。

## 三项审查结论

1. **领域语义保持。** CompanyId 由当前 CompanyCatalog 的代码排序分配，不能跨目录借用数字身份。全默认目录 C-600101 的 id=3，而 focused 会话仅含该发行人，其实际 id=0。原测试用 id=3 查询仅含实际公司 id=0 的 closing/report authority，误判没有结账。本改动从已存在的 `controlled.company_operations.catalog()` 获取受测身份，不改生产会计、自然日、披露排期或 A 股交易规则，无新增制度实现需要外部法源。
2. **必要且最小。** 只替换本 required-long 的身份来源。`closing_before` 仍在交易 step 之后；严格 `>`、显式 `end_civil_day`、同一 Session 的完整年末到年度披露循环、最后报告和存档强断言、ignore/required-long 分类均保留。没有添加数字常量、挪动 before、跳过长例、修改共享 helper 或顺修其它场景。
3. **本次边界充分，无剩余精确集成阻断。** 两条运行路径的公开 API 探针都显示：step 前后 actual Annual versions=0、civil date=2030-12-31；显式 civil 日结后 actual versions=1、日期=2031-01-01，错误目录查询仍为0。故不是 step 提前封账。原 required-long 在修正后完整推进到2031-03-25披露，版本严格增加与年度报告正资产断言都执行。额外短测7/8，失败已由原 binary 同模式复现，不能称全部相关测试通过。

## 证据绑定与限制

原 full9 inventory 的 company_scenarios artifact 与 full6 冻结 binary SHA 均为 `8883654dcfcedb88d189c817b20a153f2e89ed9840d2a7792f3b112adb78d81c`。原精确 required-long 重现 lifecycle.rs:102 的严格增加失败，3.14s/exit101；原红日志（历史路径：`../../.tmp/year-close-authority/original-red.log`） 与双路径探针支持上述输入→错误结果链，不凭静态猜测归咎生产。

最终测试 binary 用 fresh rustc 直接链接只读正式 engine rlib 构建，**不是重新编译 engine 或 clean-main 全构建**。构建前后绑定1679项源码、978项只读依赖，只有 lifecycle 一项源码变化；128 codegen units，实采多个 opt cgu 线程。编译2.653s，含前后校验13.896s。实际新 binary SHA 为 `e4af98b5a733a30797d260b53dacc43319739abe6c9f71a336e92d1d70dd1117`，engine rlib SHA 为 `76b29b322fa16901cb9c2948f51fde9864e8d2640c667d3a2eccddf322b74b64`。首次未绑定构建副本保留但不作为交付绿证。

最终 执行 receipt（历史路径：`../../.tmp/year-close-authority/fixed-year-run-result.json`） 在 child 启动前记录实际命令、runtime=1、源和产物身份；外部共享300000ms进程树 deadline，1000ms收尾预留，包含前后源/产物校验与等待。实际 exit0，child wall4.607s，含校验的脚本总wall5.697s，harness自报4.20s。完整输出（历史路径：`../../.tmp/year-close-authority/fixed-year-long-bound.log`） 为 report14、2030-12、2031-03-25、versions1。此单 Session 自然日链有顺序依赖，内部 runtime 组/交易部线程已采样；不把单 harness thread 误称单线程引擎。

额外8个短例以8进程并发、每命令和case独立10000ms门禁执行，7过1败。失败为 legacy 模式 `civil_information_chain_keeps_unread_beliefs_stable_and_closed_days_tick_free` 的 line46 beliefs 字节相等断言；同原冻结 binary、runtime=0、相同精确用例同样失败，2.72s/exit101。原 binary 对照（历史路径：`../../.tmp/year-close-authority/original-legacy-short-control.log`） 已保留，未弱化断言、未顺修，不属于本补丁新增失败。正式 runtime 的3713普通通过不涵盖此 legacy 路径。

详细归因与全部失败边界见 diagnosis.md（历史路径：`diagnosis.md`）。本批准仅供该单文件精确集成，不代表 full9 已通过，也不替代后续整树 before 与完整回归。
