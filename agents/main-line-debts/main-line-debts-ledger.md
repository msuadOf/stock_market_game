# main 预存债修复批台账（债 1 web tsc + 债 2 save_contract/player_snapshot）

> 工作文件（agents 台账规范）。实施在主 checkout main（基点 765861b5），两类债各自独立提交。
> 本批前置证据：[`../main-line-reintegration/b4a-merge-record.md`](../main-line-reintegration/b4a-merge-record.md)
> §main 预存债清单、[`../main-line-reintegration/b2-side-effects-ledger.md`](../main-line-reintegration/b2-side-effects-ledger.md) §main 既有漂移登记。

## 债 1：web tsc company/report 域红（31 处）

### 基线（清 node_modules/.tmp 缓存全量 tsc -b）

31 错（B4a 台账记 32=31 app+1 node；node 面随缓存清理消失），10 文件：
report-query-context.test×7、remote-company-reports.test×5、company-query-coordinator(.ts×4/.test×4)、
report-query-context×3、remote-host×2、public-report-fixture×2、company-report-selection.test×2、
company-slice.test×1、company-reading.test×1。全部为 company_id（及连带 cursor/entity_id）string↔number。

### 权威方向裁定（依据）

- 生成面/引擎/server/wasm wire 一律数值：`PublicReportSummary.company_id: u32`、
  `PublicReportQuery{company_id: u32, cursor: Option<u32>}`、server 路由 `Path<u32>`、
  wasm `serde_wasm_bindgen` 直序列化（数值）；`PublicationId(u32)`（cursor=其数值形态，
  next_cursor=十进制文本）；scope `MemberId(u32)`（与 CompanyId 数值同源，groups.rs `MemberId(id.0)`）。
- W3 web 层（coordinator/fixture/store/校验器）把 company_id 当字符串——W3 合并时生成面已是
  number（2bd74818 树实证），W3"tsc 0 错"受增量 tsbuildinfo 掩盖（B4a 台账怀疑成立）。
- 对齐架构 = ADR 0041 阶段二 §6 摄入/请求翻译（与 stock-identity.ts 同式）：
  - 视图身份（store/UI/协调器入参）保持字符串，canonical 形态 = 数值 CompanyId 十进制串
    （与 AccountId canonical 化同式；披露事件摄入层 stock-identity.ts:322 既有形态即此）。
  - 请求边界（协调器/ReportQueryContext/remote-host）译数值；显示边界（ReportNotes 报告范围）
    经目录表译回代码串。

### 改动（提交 fix(web): company/report 域 tsc 类型对齐）

- 新增 `apps/web/src/config/company-ids.ts`+测试：代码串↔数值 CompanyId 唯一翻译表
  （publicCompanies 代码升序取下标对齐 IssuerRegistry 分配；测试钉住 9 码序）。
- `company-query-coordinator.ts`：companyId/cursor 规范十进制串→数值 wire（不合规显式失败）；
  响应归属比对改数值。
- `report-query-context.ts`：内部改数值公司身份（wire 形态）。
- `remote-host.ts`：URL/查询参组装数值化（String()）。
- `public-report-normalize.ts`：company_id/scope.entity_id 运行时校验 text→u32 数值
  （原校验必拒真实 wasm/server 载荷——同因运行时债，随本批收口）。
- fixture/6 测试件对齐；ReportNotes 显示译码串（用户可见语义不变）。

### 验证

- tsc -b：31 → **0**。
- web 单测（run-web-tests.mjs）：7→8 分片全绿 **0 失败**（897 过；新增 company-ids 钉序测试）。
- 10s 分片 deadline 偶发 kill（heavy vite-ssr 分片）＝main 既有环境现象：stash 干净 main
  对照同现（0 测试失败+1 deadline kill），非本批引入。

### 登记（非本批引入/未越权处理）

1. **UI→wire 接线未闭合（W3/B4f 域，B4a 台账既有登记）**：CompanyPanel/LocalRefreshViews
   传目录代码串（"C-600101"）为 companyId 入协调器；现协调器显式拒绝非规范十进制串
   （原先送到 wire 被宿主拒，同样不可用，无功能回退、失败更显式）。接线需 UI 边界译码。
2. **存档 groups 数值 serde vs web 字符串 schema**：引擎 SaveSlot.groups serde=数值
   CompanyId，而 web `save/schema/company/groups.ts` 校验非空字符串、引擎侧
   `#[ts(type=…GroupStructure[])]` 覆写锚定该字符串 schema——真实 wasm 存档过
   parseGroups 必拒（latent，web 存档测试均用字符串 fixture 未暴露）。同族债，
   属存档契约域，需独立批处置（改 web schema 为数值+引擎 ts 覆写同步，或引擎侧定夺）。
3. **根 bindings/ 目录陈旧**：git-ignored 未跟踪（TS_RS_EXPORT_DIR 实际指向
   apps/web/src/types/generated），含旧 string 形态 PublicReport*，易误导，建议清理。

## 债 2：save_contract×5 + player_snapshot 红

### 基线（复跑与 B2/B4a 台账逐名一致）

save_contract 33 过/5 败：tampered_calendar_policy、duplicate_operating_due_id、
zero_and_overflowing_plan_horizons、invalid_bank_ecl_with_company_context、
invalid_industrial_loan_facts；player_snapshot（session 二进制）1 败。

### 逐例判定与处置

| 例 | 判定 | 依据 | 处置 |
|---|---|---|---|
| tampered_calendar_policy | **真 bug（修代码）** | 篡改 save.civil_clock.policy digest：persistence.rs candle_calendar 校验点经 From 直落 `SessionError::Calendar`，而会话恢复路径（CivilClock::from_parts）同篡改产出 `CivilClock(Calendar(..))`——同一政策两校验点错误域不一致；session_boundary.rs 既有测试均锚 CivilClock 域 | candle_calendar 两处 CalendarError 显式 `map_err(CivilClockError::from)` 归域；测试零改动转绿 |
| bank ECL with company context | **错误契约收口（修代码+码断言）** | 8447ecd6 数值化把文本错误压成 fault2096 ctx[0,0]，站点注释本就登记 ctx[0]=CompanyId 待 C 批 | 实现 2090-2096 家族 ctx[0]=数值 CompanyId（循环 id 在场，Insurance 既有测试仅码断言不受影响）；测试改码+ctx[0] 断言（定位强度≥原文本 contains） |
| duplicate due id / plan horizons / industrial loans（×3） | **测试过时（改断言，不弱化）** | 语义校验在 Deserialize 内以 serde custom error 拒绝且确实触发（探针实证"duplicate due id 1346"等）；8447ecd6 后 decode_save_slot 统一 fault2080（站点登记 ctx 数值化待 C 批），项目方针=首错测试文本 contains→码断言（ADR 0041 阶段二 §1），该批迁移漏了这 5 个集成测试 | 边界断言改 fault2080 码；**新增库级 serde from_value 文本定位断言**（钉住"是哪条校验拒绝"，精确度不降）；baseline() 加未篡改基线 JSON 解码夹具完整性控制（fault2080 可归因于篡改） |
| player_snapshot | **测试过时（改断言）** | 073d03e6"统一玩家默认入场资金为一百亿元"改 GameConfig::proposed_defaults starting_cash=1e12 分，测试硬编码 10_000_000 未随迁 | 断言改与 fixture 配置同源（proposed_defaults().starting_cash），杜绝再漂移 |

### 验证

- save_contract：**38 过/0 败**（原 33/5）。
- session 二进制：player_snapshot 绿；余 1 败 `full_day_accounting_rejects_a_missing_day_boundary`
  =B4a 台账登记"full_day 双亲分层差异"预存红（stash 干净 main 对照同败，`休市股 0 出现分钟桶`
  InvariantViolation，非本批引入，不在本批两债范围）。
- engine lib 全量、wasm32 check（engine+web-wasm）、web tsc：见下"回归对照"。

### 登记

- fault2080 语义类（serde custom 校验拒绝）逐校验专用码 = C 批待办（站点注释既有登记）；
  本批测试面以库级 serde 文本断言保定位，边界码断言保拒绝事实。
- fault2090-2096 ctx[1]（错误种别枚举/字符串 id 数值化）仍待 C 批。
- full_day 双亲分层差异（休市股分钟桶 InvariantViolation）待独立批归因。

## 回归对照

| 面 | 基线 | 本批后 |
|---|---|---|
| engine lib | 2205/0 | **2205/0**（一致） |
| engine tests 面 | 3381/8（desk r09 偶发 1+双亲预存 7） | **3391/1**：save_contract×5+player_snapshot 转绿；唯一余红 full_day 双亲（stash 干净 main 同败，既有登记）；desk r09 本轮未触发（满载偶发）；零新增 |
| wasm32 check（engine+web-wasm） | 通过 | 通过 |
| web tsc | 31 错 | **0** |
| web 单测 | 0 失败 | 0 失败（897 案例分片求和；含新增 company-ids 钉序测试） |

提交：`55a9d6df`（债 1，15 文件）、`2a4db2bc`（债 2，4 文件）。

## 过程登记

- 排障探针：persistence.rs fault2080 站点临时 eprintln 实证语义校验真实触发
  （"duplicate due id 1346" 等），诊断后已回退，不入提交。
- `.git/index.lock` 陈旧锁（0 字节、23 分钟、无在途 git 进程，疑本批 stash
  竞态残留）：按"等待重试勿盲删"先轮询+进程核验后移除，提交正常。
- 既有 stash（stray-w3-web-files-in-b4a / pre-rewrite-main-local-changes）非本批
  产物，未动。
