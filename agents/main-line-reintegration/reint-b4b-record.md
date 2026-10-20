# B4b 批落地记录（SessionSetup/save/构造期装配+spine 公司状态）

> 实现 agent 产出台账。分支 `feat/reintegration-b4b-session-assembly`
> （worktree `.worktree/reint-b4b`），基 main `fd5f210a`（含 B4a 公司域）。
> 计划锚点：`reintegration-campaign-plan.md` §B4b（行 146-153）；
> 素材：`reint-company-coupling.md` 映射 #7/#11/#12/#13、B4a 台账两份。

## 提交序列

| 提交 | 内容 |
|---|---|
| `1383b859` | feat(engine): B4b 主刀——容器+字段族+构造期装配+save/restore（49 文件） |
| `827849df` | test(engine): 验收测试+夹具+save_contract 信封对齐 |
| `66f5bf68` | chore(engine): ts_rs 生成物同步（SessionSetup.ts +51 行） |
| （本提交） | docs(agents): B4b 台账 |

## 设计决策（与 18725d07 的差异及理由）

### 1. 公司字段族全部可缺席（serde default + skip_serializing_if）

旧 main 的公司字段是**必填**（无 serde 默认，缺席档显式拒绝）——那是
「公司线恒在」的世界。新树（B4b 之前）存在大量无公司线的局（全部既有
测试与生产默认局），验收要求「无公司配置的旧局零行为变化」：

- `company_system/dividend_tax_mode/flat_withholding_bp/par_value_per_share/
  report_frequency`：`Option` + `#[serde(default, skip_serializing_if)]`；
- 三个开关 bool：`#[serde(default, skip_serializing_if = "std::ops::Not::not")]`；
- **旧局存档字节零变化**（新字段全部缺席即不序列化；验收测试逐 key 断言）；
- web `parseSetup`/`parseStrictSaveEnvelope` 的 exact() 键表因此不破——
  公司局存档的 web 解析面留给 B4f（ROOT_KEYS 扩展+schema 文件）。
- 严格性保留在**校验链**而非 serde 形态：公司局（company_system=Some）内
  tax mode/par value 必填、三态契约、印花税配对、面值域全部显式拒绝；
  残缺配置（company_system None 却携带公司字段）以残缺位图拒绝（fault1048）。

### 2. ts 形态：setup 层真导出，权威状态层 ts(skip)

- SessionSetup 新字段经既有生成类型（CompanySystemConfig/CashDividendTaxMode/
  ReportFrequency/Money）导出；bool 开关用 `#[ts(as = "Option<bool>")]`
  保持 web `DEFAULT_SETUP` 字面量合法（不新增 tsc 错）。
- SaveSlot 的 `company_system`/`corporate_actions` 按本树既有公司域权威
  字段惯例（closing_registry/public_library/disclosures 同款）`#[ts(skip)]`：
  web 存档 schema（save/schema/company/system.ts + corporate-actions.ts）
  与解析器随 B4f 接线，届时翻成 ts(type=import) 并恢复 ts(export)。

### 3. `SessionCorporateActions` 只取容器子集

取回：13 字段容器 + 6 个 session 级子结构 + `configure_registry`/
`configure_cash_dividend_tax_book`/`validate_registry`（勾稽）+
`taxpayer_identity_of_kind`。**不取**（分批边界）：日终 12 步链与日终勾稽
`validate`（B4c）、审批/认购命令与 AdminRight（B4d）、查询视图族与
ts(export)（B4e/B4f）。

### 4. 数值身份适配（B4a 约定延续）

- `issuer_specs`：代码升序清单 `enumerate` ⇒ `CompanyId(index)`（配置侧
  数值 id = 代码升序下标，IssuerRegistry::new 同款约定）；
- 账户持仓按数值 StockId 键（ADR 0041）⇔ 公司域 StockCode 键：装配函数内
  经 `StockCatalog` 互译（`id(code)` / `code(id)`）。

### 5. spine 状态进入 tick 哈希

`company_system`/`corporate_actions` 进 `business_state_hash` 字段流
（缺席局=null 定界字段）。理由：权威状态必须参与哈希，否则 B4c 的
回滚等价验证（R2 锚）对公司状态分歧失明。既有哈希断言全部是前后自比
（无字面值锚），不受字段流变化影响；step_skeleton 特征锚覆盖的是
events/snapshot/save 字节（旧局 save 字节未变），亦不受影响。

### 6. B3 遗留 seam 就地恢复

`memberships.rs::validate` 的 B3 裁剪注记（fault2020 账户集合）按
`setup.issuer_repurchase_enabled && company_system.is_some()` 追加回购
专户（id = npc_count+1）——注记原文点名「随 B4b 恢复」。
`company/assembly.rs` 的 B4a 注记（报告频率固定 Quarterly）改接
`setup.report_frequency`（缺席回落 Quarterly=既有行为）。

## 落地范围

| 面 | 内容 |
|---|---|
| `session/corporate_actions.rs`（新） | 容器+装配期写入面+容器 serde 契约测试 |
| `session/company_assembly.rs` | +`assemble_company_system`/`issuer_specs`（fault1162/1163） |
| `company/assembly.rs` | `CompanyAssemblyContext.report_frequency` 接线（双调用点） |
| `account.rs` | `AccountKind::IssuerRepurchase` 变体 + 9 处穷尽匹配适配（attention×2/initial_allocation/策略 decide×2/工厂/sample_cash/tail_exponent/quote_lifecycle） |
| `session.rs` | SessionSetup 8 字段+`validate_company_fields`（fault1048-1057）；state 两字段+FieldId×2+clone/commit+`accounts_insert_with_history_reads`；`assemble_corporate_foundation`（fault1058-1062）；SaveSlot 两字段；save_projection/restore（fault1063/1064）；读面访问器 `company_system()`/`corporate_actions()` |
| `session/persistence.rs` | 在场性配对 fault2101 |
| `session/memberships.rs` | fault2020 期望账户集补回购专户（B3 注记恢复） |
| `session/account_book.rs` | `has_missing_npc_strategy` 放行回购专户（无策略非玩家账户） |
| `session/hash.rs` | 状态哈希字段流 +2 |
| 测试 | `tests/session_company_foundation.rs` 13 例（三态链/装配/roundtrip 逐位/旧局零变化）+ `test-support/simple_company.rs` 夹具恢复（数值化适配）；37 文件 SessionSetup 字面量机械适配 |

## 验收结论（2026-10-20，机器负载 ~30% 时段）

| 面 | 结果 |
|---|---|
| 验收测试 `session_company_foundation` | **13/13 绿**：三态链 5 例（1048/1051-1056）/装配 4 例（专户开立+零分配/名册全量+法定事实/关闭开关等价）/roundtrip 逐位（领域 Eq+wire 字节双面）/旧局零变化 2 例（存档逐 key 缺席+restore 等价） |
| engine lib（std-thread 默认） | **2203 passed / 0 failed** / 11 ignored（基线 2202 → +1 容器 serde 契约） |
| engine 集成面（68 binaries, --no-fail-fast） | **3228 passed / 6 failed**——6 例全部为基线预存：save_contract×4（decode 层 fault2080 文本折平 vs 旧文本断言，C 批数值化债，双亲同红）+ session×2（player_snapshot 现金分层/full_day should-panic 命中别处，B4a 合并记录登记的双亲分层差异）。**本批修复 1 例**：tampered_calendar（信封对齐，见下） |
| wasm32（RUSTFLAGS="" worktree 覆盖） | engine 与 web-wasm 双包 `cargo check` 通过 |
| web tsc（清 node_modules/.tmp 真实缓存全量） | **32 错 = 当前 main 31 错逐位同集合 + vite.config.ts(1,29) 1 处**；该处为基点分叉（fd5f210a 无 4ee396be 的 vite COOP/COEP 恢复提交，基点版 `PluginOption` 未使用）——**本批 0 新增**，合并新 main 自然消解 |
| web 单测（run-web-tests.mjs） | 919 过 + 2 败（wasm-worker-ownership×2）；**pristine fd5f210a worktree 同样 2 败**=worktree node_modules 符号链接环境产物，非本批引入（main 本体同命令全过） |
| server/desktop/native-store/engine-gpu | `cargo check` 全绿（desktop 含 --all-targets） |
| std-thread-proto | 基线预存 18 错（StockId 迁移未跟进的旧原型包，fd5f210a 与现 main 同破）——非本批引入，未越界修 |
| **B 探针对照**（driver_semantics_probe，taskset 96-111 钉核，warm 后交错对） | **3000 tick 主口径 5 对：中位 P 1279.8 / B 1280.0（+0.02%），均值 -0.09%；1000 tick 口径 6 对：中位 -0.37%**——均在 ±2% 门内（未钉核裸跑摆幅 ±10-40%，已弃用；binary SHA pristine=5e6797349fe4cb00 / branch=7c836a9bf7eb470b；守恒与事件数两臂逐位一致） |

### save_contract 5 失败的处置（B4a 合并记录指派 B4b）

- **修复 1**：`tampered_calendar_policy_is_rejected`——拒绝点自 restore 层
  （`CivilClock(Calendar(DigestMismatch))` 信封）前移到 decode 层
  （`SessionError::Calendar(DigestMismatch)` 直达，B4a 记录明言「错误从
  restore 层提前到 decode 层」）。类型化载荷（field/stored/computed）两
  信封完整保留；测试改断言双信封形态（等价强度，非弱化）。
- **保留红 4**：duplicate_operating_due_id / zero_and_overflowing_plan_horizons /
  invalid_bank_ecl_with_company_context / invalid_industrial_loan_facts——
  四例篡改在域 Deserialize 内以**文本**拒绝（"duplicate due id" 等），
  `decode_save_slot` 的 fault2080 映射按 string-free 契约折平文本（站点注释
  自登记「槽0 待 C 批数值化（原文本 "{error}" serde 错误）」）。诚实修复
  需按域分配 decode 层错误码（C 批 B 项「必须归零」的既定工作），本批
  强改只能弱化断言到裸 2080（不可接受）或破坏 string-free 契约（不可
  接受）。**双亲同红证据**：B4a 合并记录 §声明复验（pristine/B4a tip/merged
  三方同 5 败）+ 本批 diff 未触碰 decode_save_slot 映射与四测试。


## 遗留与交接

- `packages/std-thread-proto` 在基点 fd5f210a 与当前 main（a341d9ba）均
  18 处编译错（StockId/StockCode 迁移未跟进的旧原型包）——**预存破损，
  非本批引入**，未修（越界）；--workspace 全目标门禁因此不能整体跑绿，
  各包单列验证。
- 公司局存档的 web 解析面（ROOT_KEYS/parseSetup 扩展 +
  save/schema/company/system.ts + corporate-actions.ts）→ B4f。
- `GameSession::new` 构造期 `report_frequency` 只影响前史公开库排期；
  公司系统线的 Simple 披露发布（`publish_simple_scheduled` 构造期调用）
  属披露面融合 → B4e。
