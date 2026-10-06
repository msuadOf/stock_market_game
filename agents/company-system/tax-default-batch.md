# B2 批次：默认扣税 + 开局税务选项 + 宿主/UI 入口（2026-10-06）

## 产品决策来源

2026-10-06 用户 verbatim 要点：「新局默认用户交易都扣税，还可以选择用大A的方式扣税
（个人扣税、机构/企业另外算……企业部分先留下抽象层接口，可以先接通simple模型……
留好后续复杂开发的空间），还可以配置为不扣税。此外，分红要扣税」。

## 实现记录

- `SessionSetup.dividend_tax_mode`：`CashDividendTaxMode::{IndividualPublicMarket, Exempt}`，
  必填、无 serde 默认；旧档缺失该字段在 engine serde 与 Web `parseSetup` 显式拒绝。
- 默认模式在 `GameSession::configure_share_registry` 成功后于同一候选副本上为个人身份
  持有人（Player + Retail）自动走既有 `configure_cash_dividend_tax_book` 开账；Inst/Hot
  保持 TreatmentNotConfigured；任一步失败不留半配置状态。
- 身份分类权威映射：`corporate_actions::taxpayer_identity_of_kind`。
- 查询面：`GameSession::account_dividend_tax_status`（模式+身份+各登记证券税账状态）；
  wasm owner 隔离导出 `owner_dividend_tax_status` / `owner_dividend_tax_outstanding_views` /
  `configure_dividend_tax_book`（显式装配期命令，开局后拒绝错误完整上抛）。
- Web：`TaxModeInput`（新局面板，默认大 A）+ `DividendTaxPanel`（CompanyPanel 体系内，
  needs_funds 显式提示）；worker-host 经 `dividendTaxStatus` / `dividendTaxOutstanding`
  可选宿主能力接线，Tauri/远程宿主暂不支持并显式提示。

## 存档 fixture 迁移

- 主/休市/最小三档均用 release Engine 正规重生成（`.tmp/company-system/tax-default/`
  的 producer 与日志），setup 显式携带 `dividend_tax_mode: "IndividualPublicMarket"`；
  未配置名册时 `dividend_tax_books` 为空数组。closed-day generator 的空数组校验补入
  `dividend_tax_books`。`current-company-slice.json` 从新主档 `company_system` 精确投影。
- 主 producer 复用旧档 setup+seed（666959854，2 交易日 × 60 tick）；与旧档轨迹差异
  仅来自旧 producer 的日结顺序差异：把新档去掉新字段后在 pristine HEAD（02d68eda）
  release Engine 上 restore+resave，与本批 Engine 输出深等（drift-check 证据在
  `.tmp/company-system/tax-default/pristine-resave.json`），证明本批未改变模拟行为。

## 边界与后续

- 装配期后经二级市场进入登记的股东不自动补开个人税账（既有守卫边界，已在
  trading-rules.md 登记）。
- 企业/机构计税未实现：`TaxpayerIdentity::NonIndividualPending` + `DividendTaxProfile`
  既有变体为扩展位。
- Tauri/远程宿主税务查询、企业税 simple 模型接线留待后续批次。
- 引擎 lib 测试在本批基线（main @ 02d68eda）上存在 257 个与本批无关的既有失败；
  本批新增用例全部通过，且失败集合与 pristine 基线完全一致（零新增失败）。
