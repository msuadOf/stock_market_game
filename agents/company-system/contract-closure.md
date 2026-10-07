# 共同契约收口台账（F 批，2026-10-08）

工作目录：worktree `wf_58e8fbc7-359-1`，基线 main `dc311164`（合并 N1 三层税制与 N3 汇总账簿）。四个红→绿提交：`e1bf6f12`（错误四分类）、`590d1cf0`（能力面+解释查询）、`3a3ae68f`（session 共同契约视图）、`dd3a78db`（web 接线）。与 N2a 名册批并行：本批不触碰 session 创建／名册装配／偏好默认／税账开账。

## 范围与定位

审计（`checklist-current-code-audit.md` §2.1/2.4/2.5/2.6）认定缺口：capabilities 只返回静态布尔位＋`cash_settlement=false`；解释只存内部 history 无上层读取入口；错误大量归 `Invalid` 无完整分类。本批按 R4 落「消费者入口的最小公共合同」：不照搬 Q14 §3 拟议函数名、不开放内部账簿。

## 交付内容

1. **错误四分类附加面**：`CompanyErrorClass`（InvalidInput/BusinessCondition/UnsupportedOperation/SystemState，ts-rs 导出）+ `CompanySystemError::classification()`；新增 `InvalidInput`/`BusinessCondition`/`SystemState` 三个 **display 与 `Invalid` 同形** 的变体（不改变行为只改分类面），`SimpleFinanceError::classification()` 委托同口径。调用点逐步映射：未知公司（system.rs×7、state.rs）→ InvalidInput；复业条件 → BusinessCondition／复业参数 → InvalidInput；恢复勾稽（system.rs validate_* 全部 format! 站点、state.rs validate() 全部）→ SystemState；`日期必须连续推进` → InvalidInput。测试每类 ≥2 例行为路径（`company::error_classification_tests`，9 项）。
2. **能力面结构化**：`CompanyCapabilities` 升级为完整消费视图（静态位＋每股面值〔Money 分字符串〕＋现行总股本＋注册资本/可分配利润快照〔元字符串与公开报表同口径〕＋未完成方案〔五类：阶段超集+关键日期中文标签〕＋各行为业务条件快照＋本人权利摘要〔owner 隔离〕）；公司侧事实由 `CompanySystem::company_facts` 供给，完整视图只由 session 组装（company 域不产出半填充视图）。不可用显式 reason 不填零。原 `CompanySystem::capabilities` 静态方法删除，`simple::tests` 调用点更新。
3. **解释查询**：`CompanySystem::period_change_explanation(company, period_end)` 复用既有 history（非周期末日=InvalidInput、未结算/早于前史=BusinessCondition、范围内缺失=SystemState）；`PeriodChangeExplanation`/`PeriodAmounts`/`TrendSegment`/`AnnualTrendState` 补 ts-rs 导出。
4. **session 只读视图**（`session/company_contract_views.rs`）：`company_capabilities`、`owner_rights_offerings`（未完成配股：权证/公开配售剩余额度〔与 `subscribe_rights_offering` 受理口径同构，溢出/透支显式报错〕/窗口 BeforeOpen/Open/Closed/排队与已结算认购；Settled 终态不出现）、`owner_flat_withholding_receipts`（仅 Flat 模式，否则显式拒绝）、`company_period_explanation` 委托。readiness 五类固定序；「满足」不承诺受理。
5. **web 接线**：wasm 四导出（owner 固定 AccountId(0)，公司身份 UTF-16≤64 与偏好台账同口径）→ wasm-worker 四消息 → worker-host 严格 parser（`host/company-contract-views.ts`）→ engine-host/provider/LocalRefreshViews → `CompanyContractPanel`（CompanyPanel 内只读区；宿主不支持逐区显式提示）。

## 验证

| 项 | 结果 | 证据（`.tmp/company-system/contract-closure/`） |
| --- | --- | --- |
| 错误分类红→绿 | 先编译红（缺 API）后 9/9 | `classification-red-errors.log`、`classification-green.log` |
| 能力面/解释红→绿 | 先编译红后 9/9（含元字符串、不填零负例） | `capabilities-red-errors.log`、`capabilities-green.log` |
| session 视图红→绿 | 先编译红后 9/9（有/无具名权利、窗口前后、排队→已结算、Settled 隐藏、Flat 过滤与非 Flat 拒绝） | `session-views-red.log`、`session-views-run2.log` |
| 受影响 engine 组 | company:: 436 过/3 败（败者=pristine main 既有，见下）；rights 13、tax-mode 20、simple-session 36、preferences 16、corporate_actions 19 全绿（并发） | `company-after-capabilities.log`、`affected-*.log` |
| 既有失败基线对照 | `company::operations::day` 三项失败在 pristine `dc311164` worktree（/tmp/f-baseline-check）复跑逐项一致，与本批无关 | 终端复测记录 |
| typegen | `cargo test -p engine --lib export_bindings` 176 项通过；`check-generated-types` 提交后通过 | `typegen.log` |
| tsc / oxlint | `tsc -b --force` 0 错误；本批 4 个新 web 文件 oxlint 0 警告 | `tsc.log`、`oxlint.log` |
| web 新增测试 | parser 16/16、panel SSR 4/4（10000ms case+命令门禁） | `web-parser-tests.log`、`web-panel-tests.log` |
| workspace check | `cargo check --workspace --all-targets --exclude stock-market-game` exit 0 | `workspace-check.log` |
| web 既有失败对照 | `wasm-worker-ownership.test.ts` 2 败在 pristine baseline 同样失败（环境性既有，非本批引入） | 终端复测记录 |
| web 全量基线对照 | 本批全量（8 shard）与 pristine `dc311164` 全量失败集均受「首败即中止 sibling」影响而不稳定且互有出入；对全量中出现的全部嫌疑文件逐一单跑，两侧结果**逐文件逐项一致**（serde-normalize 6过/3败、wasm-save-protocol 1过/1败、report-correction-transports 1过/1败、wasm-worker-failure 12过/0败、worker-host 33过/0败、wasm-worker-ownership 0过/2败）——本批零新增失败 | `web-full-suite2.log` 与终端单跑记录 |

时限纪律：普通测试 `run-with-deadline.mjs 10000`；编译/typegen/tsc 用 `run-long-validation.mjs 300000`；受影响组以多进程并发执行。

## 已知边界与遗留

- **Invalid 迁移余量**：`simple/period.rs`、`growth.rs`、`config.rs`、`environment.rs`、`preferences.rs`、`identity.rs`、`persistence.rs` 的配置校验类 `Invalid` 未迁移（按四分类默认落 InvalidInput，语义大体准确）；`system.rs` 中三处 `.map_err(|e| Invalid(e.to_string()))`（含 `report_availability`）未迁移。后续批次继续逐步映射。
- **SessionError::InvalidSetup 泛滥**（session 域）不在本批四分类面内；`session::corporate_actions` 各错误枚举（CashDividendError 等）的跨域分类留待后续。
- 拆股 readiness 无状态级前置条件（碰撞预检依赖具体方案输入），快照恒 ready——已注释说明，不冒充可判定。
- 回购 Completed（未注销）计入未完成方案；注销后 Cancelled 为终态。
- wasm crate 内 owner 过滤直接单测与 S2 遗留缺口相同（engine session 层已覆盖逻辑，wasm 导出为薄委托），沿用既有遗留登记。
- Tauri／远程宿主查询面未接线（UI 显式提示）；`engine-host` 四方法为可选契约。
- 本机 Node 25 与 corepack-pnpm 脚本不兼容：typegen 直接 `cargo test`、tsc/oxlint 用主仓 node_modules 绝对路径、web 测试 `node --test` 直跑；未修改工具或放宽门禁。

## 文档同步

- `docs/trading-rules.md` 新增「公司共同契约公共查询面」节；`docs/company-actions-design.md` 新增 §5 查询合同表。
- `implementation-checklist.md` §2.5/2.6 追加 2026-10-08 F 批对账注记。
- `current-handoff.md` 验证表新增 F 批行、「未完成边界」更新能力面表述。
