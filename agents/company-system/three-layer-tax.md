# 三层税制台账（N1 批，2026-10-08）

## 决策与范围

用户 2026-10-08 三层税制产品决策（ADR-0040 三层修订）：「简税默认+大A可选（按原话三层）」
「简税比例 10%（推荐）——分红到账时直接扣，无持股期档位；开局参数可编辑，机构不另算」
「不扣税：连印花税也免（佣金/过户费照付）」。按 2026-10-07 无兼容原则整体替换：
不分新旧版本、接口破坏性替换、旧实现整体删除、命名禁止版本后缀、零兼容层、旧档显式拒绝。

## 命名决策（最终名与理由）

- `CashDividendTaxMode::FlatWithholding`（简税）：语义是「付款日按平比例代扣」
  （withholding at payment at a flat rate），无持股期/无档位含义直白，无版本痕迹。
- `CashDividendTaxMode::AShareIndividual`（大 A 方式）：会话级产品选项，指「按大 A
  个人口径计税」这一开局选择。与既有 `DividendTaxProfile::IndividualPublicMarket`
  （税账的具体计税身份/口径，语义是「个人公开市场差别化」）刻意区分：前者是玩家
  选的模式，后者是税账事实的口径名，二者是不同层概念，均无兼容后缀。
- `CashDividendTaxMode::Exempt`（不扣税）：保留既有名，语义不变（个人股息税事实
  不产生），但按新决策**扩展为同时免征卖出印花税**。
- `SessionSetup.flat_withholding_bp: Option<u32>`：按任务处方采用 setup 独立字段而非
  枚举内嵌数据——三态契约（仅 Flat 必填、其他模式显式拒绝携带）在 engine
  `SessionSetup::validate` 与 Web `parseSetup` 同构实现，可对「非 Flat 携带比例」
  写出显式拒绝测试；serde 缺失键 ⇔ null（非 Flat 旧形态等价不携带）。

## 实现口径登记

1. **枚举与契约**：三变体如上；旧值 `IndividualPublicMarket` 在 serde 与 Web oneOf
   两端均为未知变体显式拒绝。比例合法域 0..=10000bp——上限是**原子代扣不变量**的
   要求（代扣额 ≤ 税前应得 ≤ 到账后现金），超过即拒绝。
2. **Flat 代扣模型（更简模型，如实登记）**：付款日对名册每位**账户**持有人（玩家/
   散户/机构 NPC，机构不另算）在贷记税前应得的同一事务内立即扣收
   `half-up(gross × bp / 10000)`；代扣额 ≤ 税前应得 ⇒ 扣收必然成功，**不存在余额
   不足的部分收缴与追缴情形**（任务允许「对齐既有部分收缴+追缴先例或按更简模型
   如实登记」，本批选后者并在 trading-rules 登记）。无持股期、无税账 FIFO、卖出
   不补税；**不创建不持久化任何 `CashDividendTaxBook`**（Flat 档 `dividend_tax_books`
   恒为空数组）。代扣事实 = `FlatWithholdingReceipt`（严格持久化，含 rate_bp 冻结），
   `DividendTaxStatus::FlatWithholding` 标记到账回执。**边界**：外部具名持有人无游戏
   账户，不代扣（与 AShare 模式同一边界）。Flat 模式显式 `configure_cash_dividend_tax_book`
   被拒（防双重计税；Exempt 保留既有入口，不对称是刻意的）。
3. **印花税门禁**：`SessionSetup::validate` 单一入口强制配对——`Exempt ⇒ stamp=0`、
   `Flat/AShareIndividual ⇒ stamp=0.0005`。费用管线（`GameConfig` 传播的结算、名义
   费用瀑布、NPC 决策估算、恢复重放审计）无需各自门禁，不存在第二条计费路径；
   佣金/过户费不受影响。真实卖出成交断言三模式差异（Exempt 印花税 0 且佣金>0；
   Flat/AShare 印花税>0）。
4. **恢复勾稽（engine `SessionCorporateActions::validate` + Web parser 同构）**：
   非 Flat 模式不得携带任何代扣回执或 FlatWithholding 状态；Flat 模式税账必空、
   每位账户持有人到账恰有一条金额一致回执、`withheld` 可按 setup 比例 half-up 复算
   （BigInt 整数运算与 engine `flat_withholding_cents` 同式）、`rate_bp` 与 setup 一致。
   既有「到账回执 tax_status 推导」两处按模式扩展（Flat ⇒ FlatWithholding）。
5. **Web**：`parseSetup` 三态严格（缺键⇔null 与 engine serde 对齐；越界/非整数拒绝）；
   `TaxModeInput` 三选一（默认简税）＋比例输入（百分比↔bp，非法输入不写草稿）；
   `handleNewGame` 组装 `flat_withholding_bp`（非 Flat 恒 null）并按模式归一
   `stamp_tax_rate`（Exempt⇒0，其余⇒DEFAULT_SETUP 基线）；DividendTaxPanel/
   host/dividend-tax 标签与 oneOf 域三变体化；DEFAULT_SETUP 默认 `FlatWithholding`
   +1000bp（草稿层提供默认，非 serde 默认）。
6. **fixtures**：三个 producer（main/closed-day/minimal）改 Flat+1000bp、守卫补
   `flat_withholding_receipts` 空数组断言；由本树 release Engine（rustc 直连 rlib
   并行编译）正规重生成，内置守卫＋`ProtocolSession::restore` resave 深等全过，
   company 切片自新主档 `company_system` 精确投影（与旧切片不等源于 main 自
   上次生成后的正常演进，非本批语义漂移）。
7. **引擎测试迁移口径**：税无关 setup（不配名册/不分红）→ `FlatWithholding{1000}`
   （行为等价：无分红即无代扣、印花税基线不变）；个人差别化管线测试
   （company_simple/rights/split session tests）→ `AShareIndividual` 并删除装配名册后
   对玩家（个人身份）的冗余显式开账调用（自动开账已覆盖；机构账户的显式配置
   门禁用例保留）；偏好链路组 → `Exempt`+`stamp=0`（保持「不产生个人税事实」的
   原测试范围，与税务正交）。

## 验证（红→绿与全量对照）

- **红→绿**：旧枚举值 JSON 拒绝用例先红（当前实现可解析旧值，断言失败于
  `expect_err`——正确原因的红；`red-legacy-enum-reject.log`），实现后全绿
  （`dividend-tax-mode-tests-green.log`：19/19）。Flat 代扣手算断言（默认 1000bp：
  玩家 50→5、散户 30→3、机构 20→2、外部 10→0；改 5000bp half-up：21→11、7→4）、
  三向旧档拒绝（旧枚举/Flat 缺比例/非 Flat 携带比例）、restore 子树深等
  （setup+corporate_actions；全档深等不成立属既有 f64 规范化边界，AShare 对照
  用例同样不等，非本批引入）、Exempt 卖出印花税 0/佣金照付、Flat+AShare 印花税
  照收、Flat 显式开账拒绝、Exempt 保留显式开账——全部在
  `session::dividend_tax_mode_tests`。
- **engine lib 全量**：257 项失败与 pristine main（`git archive 5bdf0953` 临时树）
  **逐项一致零新增零移除**（本机 main 基线即有大量日历/休市类既有失败，
  `engine-lib-baseline-pristine-main.txt` vs `engine-lib-current.txt`；全量首跑
  258/257 的 1 项差异为 `verification_evidence::phase_timing` 计时类用例的既有
  抖动，复跑即一致）。受影响偏好组 16/16 绿。
- **engine 集成二进制**：17 个被迁移二进制逐个运行，失败集与 pristine main
  **逐字节一致**（仅耗时行不同，`int-baseline-pristine-main.txt` vs
  `int-current.txt`，各 131 项既有失败）。
- **workspace**：`cargo check --workspace --all-targets --exclude stock-market-game`
  0 error（desktop 需先构建 web/dist，属既有环境前置）。
- **typegen**：`cargo test -p engine export_bindings` 生成/更新 5 个绑定
  （CashDividendTaxMode/DividendTaxStatus/SessionSetup/SessionCorporateActions/
  FlatWithholdingReceipt 新增）；该命令在本机会话与并行批次共享 cargo 锁，超时
  前目标文件均已写出且内容核对无误（`check-generated-types` 在提交前因未提交
  状态报红属流程预期）。
- **web**：`tsc -b --force` 0 错误；受影响组（schema/market/corporate-actions/
  TaxModeInput/panel/host/app 命令）111 项全绿含新增正负例（三层三态、比例域、
  旧枚举拒绝、Flat 回执勾稽五向负例、Exempt 新局印花税归一）；逐文件枚举全量
  231 文件 13 失败文件与 pristine main **完全一致零新增**（`web-perfile-current.txt`
  vs 基线；`run-web-tests.mjs` 首败即中止 sibling，故用逐文件枚举法，先例见
  S2 批）。oxlint 改动文件 0 新警告（DividendTaxPanel 2 条 only-export-components
  为 main 存量）。
- **rustfmt**：未执行——main 本身在这些文件的模块树上已有大量格式漂移（如
  `cash_dividend_tax/tests.rs`），单文件 rustfmt 会递归引入无关重排噪声；本批
  代码按仓库风格手写。
- **fixtures**：producer 输出携带 `FlatWithholding`/`flat_withholding_bp:1000`/
  `flat_withholding_receipts:[]`，restore/resave 深等过；三 fixture + company 切片
  已安装并被 web 消费组验证。

## 独立复核与修复轮

非作者 subagent（code-review）对完整 diff（5bdf0953..86560edf）静态独立复核：
结论 pass，2 minor + 3 note，无 blocker/major；三问（大 A 语义/必要性与最小范围/
跨层一致性）均通过。五项发现已全部修复并提交（594875d4）：负数 gross 防御、
比例输入受控化+显式失同步提示、重复分支合并、validate 比例域复验、引擎侧
篡改五向负例；修复后受影响组复跑全绿（dividend_tax_mode_tests 20/20）。
复核者未运行测试（声明静态审查），动态证据以本台账验证节为准。
修复轮已由同位复核者静态复验确认：五项处置全部接受、无修复引入的回归面
（web 侧 TaxModeInput 新形态由本批 41 项动态用例覆盖）。

## 未完成边界与遗留

- Tauri/远程宿主的税务状态查询与配置入口（既有遗留，非本批范围）。
- 简税模式的外部具名持有人不代扣（无游戏账户；登记于 trading-rules）。
- `simulation_policy_id` 未变更：沿用批次先例（严格字段契约已拒绝旧档，政策身份
  语义未升级）。
- N2（自动名册批）将触碰 setup/税区，`conflict_surface` 已在结构化返回列出。
- 全档深等在恢复后不成立属既有 f64 规范化边界（`json_canonical_f64` 注释），
  本批以子树深等断言并登记。
