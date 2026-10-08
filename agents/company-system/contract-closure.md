# 共同契约收口台账（F 批，2026-10-08）

工作目录：worktree `wf_58e8fbc7-359-1`，基线 main `dc311164`（合并 N1 三层税制与 N3 汇总账簿）。四个红→绿提交：`e1bf6f12`（错误四分类）、`590d1cf0`（能力面+解释查询）、`3a3ae68f`（session 共同契约视图）、`dd3a78db`（web 接线）。与 N2a 名册批并行：本批不触碰 session 创建／名册装配／偏好默认／税账开账。

## 范围与定位

审计（`checklist-current-code-audit.md` §2.1/2.4/2.5/2.6）认定缺口：capabilities 只返回静态布尔位＋`cash_settlement=false`；解释只存内部 history 无上层读取入口；错误大量归 `Invalid` 无完整分类。本批按 R4 落「消费者入口的最小公共合同」：不照搬 Q14 §3 拟议函数名、不开放内部账簿。

## 交付内容

1. **错误四分类附加面**：`CompanyErrorClass`（InvalidInput/BusinessCondition/UnsupportedOperation/SystemState，ts-rs 导出）+ `CompanySystemError::classification()`；新增 `InvalidInput`/`BusinessCondition`/`SystemState` 三个 **display 与 `Invalid` 同形** 的变体，`SimpleFinanceError::classification()` 委托同口径。~~不改变行为只改分类面~~（修复轮更正：display 与运行时控制流不变，但**变体身份是测试可见面**——`日期必须连续推进` 迁移 `Invalid`→`InvalidInput` 曾使 `session::notices` 既有断言变红，本批内已同步更新）。调用点逐步映射：未知公司（system.rs×7、state.rs）→ InvalidInput；复业条件 → BusinessCondition／复业参数 → InvalidInput；恢复勾稽（system.rs validate_* 全部 format! 站点、state.rs validate() 全部）→ SystemState；`日期必须连续推进` → InvalidInput。测试如实计数（修复轮补齐 SystemState 真实路径后 10 项）：InvalidInput 真实路径 3、BusinessCondition 真实路径 2、UnsupportedOperation 真实路径 2、SystemState 真实路径 2（会计溢出＋不一致存档走 `validate_restored`）＋变体直构 1。
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
| 受影响 engine 组 | company:: 436 过/3 败（败者=pristine main 既有，见下）；rights 13、tax-mode 20、simple-session 36、preferences 16、corporate_actions 19 全绿（并发）；修复轮补录：**session::notices** 属四分类变体迁移受影响组（`日期必须连续推进` 断言曾变红，见「修复轮」节） | `company-after-capabilities.log`、`affected-*.log` |
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
- **四分类 display 前缀分化（后续项，本批不改）**：`Invalid`/`InvalidInput`/`BusinessCondition`/`SystemState` 目前共用 display 前缀「公司系统输入非法：」，与语义不匹配（BusinessCondition/SystemState 并非「输入非法」）；分化为各自前缀会改变全部错误文案快照与 web 展示，须单列批次评估。修复轮仅登记，不改动。
- **SessionError::InvalidSetup 泛滥**（session 域）不在本批四分类面内；`session::corporate_actions` 各错误枚举（CashDividendError 等）的跨域分类留待后续。
- 拆股 readiness 修复轮起按法定事实/面值锚判定 blocker（曾恒 ready，见「修复轮」节）。
- 回购 Completed（未注销）计入 active_plans 展示（仍是持有中的未完成方案，注销后 Cancelled 为终态）；但 readiness 唯一性预检修复轮起与受理口径一致——Completed 不阻塞新方案。
- wasm crate 内 owner 过滤直接单测与 S2 遗留缺口相同（engine session 层已覆盖逻辑，wasm 导出为薄委托），沿用既有遗留登记。
- Tauri／远程宿主查询面未接线（UI 显式提示）；`engine-host` 四方法为可选契约。
- 本机 Node 25 与 corepack-pnpm 脚本不兼容：typegen 直接 `cargo test`、tsc/oxlint 用主仓 node_modules 绝对路径、web 测试 `node --test` 直跑；未修改工具或放宽门禁。

## 文档同步

- `docs/trading-rules.md` 新增「公司共同契约公共查询面」节；`docs/company-actions-design.md` 新增 §5 查询合同表。
- `implementation-checklist.md` §2.5/2.6 追加 2026-10-08 F 批对账注记。
- `current-handoff.md` 验证表新增 F 批行、「未完成边界」更新能力面表述。

## 修复轮（2026-10-08，同日 code-review/门禁复核）

入口状态：F 批四提交后 engine lib 全量 1636 过/258 败（基线 257，唯一新增＝`session::notices` 变体身份断言）。逐项 TDD 红→绿：

1. **blocker-1 u64 wire 断裂**：`CompanyCapabilities.issued_shares` 只加 `ts(type="string")` 缺 `serde(with=canonical_u64_decimal)`——serde 实际输出 JSON number，web 严格 parser 只收字符串，真机 `companyCapabilities` 恒抛错（web 手写 fixture 曾写死字符串形态，掩盖断裂）。修复＝补 serde 属性（对齐 `OwnerEntitlementDetail.rights_shares` 模式）+ typegen 重跑（生成物仅注释 diff，wire 类型本就 `string`）+ **真实往返锁**：engine 侧 `capabilities_wire_u64_fields_serialize_as_canonical_decimal_strings` 用真实 session 组装的能力面断言 `issued_shares`/`entitled_shares` 为规范十进制字符串＋serde 两向往返等值，并与 `apps/web/src/save/fixtures/company-capabilities-wire.json`（engine 真实序列化产物）逐字段同步（漂移即红；`UPDATE_COMPANY_WIRE_FIXTURES=1` 重生成）；web 侧 `parseCompanyCapabilities` 直接解析该 fixture。wasm 导出（`to_js`→serde_wasm_bindgen）与 serde_json 走同一 `Serialize`，锁此层即锁 wasm wire（low-8 同一覆盖）。
2. **blocker-2 变体迁移弄红既有测试**：`simple/state.rs::advance_day`「日期必须连续推进」`Invalid`→`InvalidInput` 迁移（语义正确：调用方传入非连续日期属非法输入）破坏 `session::notices::tests::corrupt_acquisition_reference_rolls_back_entire_day_end` 对变体身份的 `matches!` 断言。选 a：更新断言为 `InvalidInput` 并附迁移理由注释（信息原文、状态不被破坏、哈希不变断言保持）。台账如上更正「display 与控制流不变」过强表述、受影响组补录 session::notices。
3. **必修-2 React 无限请求循环**：`CompanyContractPanel` 每渲染为 `onCapabilitiesQuery` 新建闭包，`useQueried` effect 依赖 `[query, refreshKey]` 含闭包身份 → 查询完成 setState 重渲染 → 新闭包 → effect 再触发 → 无限请求循环＋闪烁。修复＝触发键 `contractQueryTrigger`（仅「支持位＋刷新键」参与，闭包身份不参与；导出供测试）＋查询本体经 ref 取最新值；effect 依赖为具名 trigger 变量（oxlint exhaustive-deps）。防回归×2：触发键纯度（同支持位＋刷新键下两次渲染的新建闭包不再次触发；刷新键/支持位变化必触发）＋SSR 渲染期调用计数为零。**诚实边界**：node 无 DOM、SSR 不执行 passive effect，「渲染两次仅一次请求」的 effect 侧以触发键契约锁定（effect 依赖数组即该函数输出），渲染路径纯度以调用计数锁定，不以 SSR 冒充 effect 断言。
4. **建议-3 回购 readiness 反向漂移**：唯一性预检曾 `!Cancelled`（Completed 也阻塞）与受理口径 `!Completed|Cancelled` 不一致——Completed 未注销时报不满足但实际可受理。对齐受理口径＋双向测试：未完成方案阻塞（负路径）；真实成交流程推进到 Completed（未注销）后 ready=true（正向）。
5. **low-4 拆股 readiness**：曾恒 `ready=true` 与 `approve_share_split` 依赖（法定事实硬前置＋面值锚）不一致。改为：未绑定法定事实→blocker（受理无条件要求 `registered_capital_at_approval`）；无既有面值锚时按「注册资本÷发行股数整除」静态判定。测试覆盖未绑定（阻塞）与绑定且可整除（无阻塞）。
6. **low-5 面值口径**：`company_facts.par_value_per_share` 由声明链 `current_par_value` 改**已入账**口径 `outstanding_par_value`（N3 既有，回购注销同口径）——批未入账时声明链提前翻新面值会造成 `par×issued_shares≠registered_capital` 的反向漂移；不可用 reason 改为明示「已入账」口径。测试：声明未入账拆股后声明链=500 分、能力面仍不可用（同批 wire fixture reason 随之重生成，往返锁验证生效）。
7. **low-6 可分配利润收敛**：能力面一次装配 3 次查询（`company_facts` 1＋readiness 2）收敛为 1 次：`company_capabilities_view` 顶部单查 `distributable_profit`，`company_facts_with_distributable`（pub(crate) 单查询变体，`company_facts` 委托）与 `action_readiness` 共享同一结果。
8. **low-7 parser Unavailable helper**：`host/company-contract-views.ts` 三个金额事实的 Unavailable 分支抽 `unavailableReason`（形状/错误文案不变）。
9. **门禁**：`error_classification_tests` 补 SystemState 真实路径（不一致存档：移除公司状态后 `validate_restored` → `SystemState`），计数改为如实（见交付 1 更正）；`OwnerRightsSummaryView.open_subscription_remaining` 注释统一 `Some("0")`=额度用尽；四分类 display 前缀分化登记为后续项（见已知边界）。

验证（修复轮各证据日志实际位于两个 worktree 的 `.tmp`：修复前基线 `before-full.log`（1636/258）在 F worktree `.tmp/company-system/fixround/`；修复后全量 `after-full.log`（1643/257）与受影响组、typegen、tsc、oxlint、workspace、web 各日志在同源 worktree `.tmp/contract-closure-fixround/`；两份 full.log 的失败清单 diff＝仅 `session::notices::tests::corrupt_acquisition_reference_rolls_back_entire_day_end` 一项转绿、零新增——该对照由终审复核者以两份 full.log 独立复现证实；`.tmp` 证据为会话期产物不随 git 入库）：受影响组并发全绿——error_classification 10、capabilities 10、company_contract_views 13、session::notices 11、company_simple_session 36、rights_offering 13、dividend_tax_mode 20、issuer_repurchase_session 5；typegen 176 项重跑（生成物仅注释 diff）；tsc -b --force 0 错误；oxlint 本批 4 个 web 文件 0 警告；`cargo check --workspace --all-targets --exclude stock-market-game` exit 0；web parser 17/17、panel 6/6、company-report-selection 7/7。红→绿现场：blocker-1（wire 断言先红「issued_shares 必须是字符串：10000000」）、low-5（先红「批未入账时不得提前采用重锚面值」）、必修-2（先红 `contractQueryTrigger is not a function`）。终审另登记：拆股 readiness 不整除负例由后续提交 `d895ce5c` 补齐（三分支全覆盖）。
