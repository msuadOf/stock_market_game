# 玩家配股认购宿主入口与送转公开披露台账（2026-10-08 G 批）

本目录工作文件按 [AGENTS 约定](../../CLAUDE.md) 归档。本批实现审计遗留 R4/§6.6
（玩家配股认购宿主入口）与 R3/trading-rules 264-271（送转公开披露链路）两项登记遗留。
worktree `wf_722872a1-cae-1`，基线 main `2aeb9c69`，不触碰 session 装配/名册/偏好默认
（N2a）与 company/simple 财务（冻结）。

## G1 玩家配股认购宿主入口

现状（M 批后）：引擎已有 `GameSession::subscribe_rights_offering(event_id, account, shares)`
（本人真实现金、公开额度截断、`RejectedRightsSubscription` 显式拒绝回执），但三宿主均无
认购提交命令，UI 无入口。

本批接通 **本机 WASM 宿主**（Web 优先既定）：

- **engine**：`ProtocolSession::subscribe_rights_offering` 委托（宿主显式入口，与
  `configure_cash_dividend_tax_book` 同模式）；`packages/engine/tests/company_operations/payment_risks.rs`
  的 `AnnouncementContent` 穷尽 match 机械扩展新变体臂（见 G2）。
- **web-wasm**（`apps/web-wasm/src/lib.rs`）：导出 `subscribe_rights_offering(handle, event_id, shares)`
  ——owner 固定 `AccountId(0)`（与 `enqueue` 同口径）、参数 = 配股事件（非空 ≤128 字符）+
  认购股数（规范 u64 十进制字符串，与股数 wire 口径一致）；受理成功按
  「(event_id, 本人) 在排队队列中恰有一条」（engine 重复提交守卫保证唯一）回查队列返回
  `QueuedRightsSubscription` 受理回执；现金不足／窗口外／额度不足／重复提交等拒绝完整
  上抛（JsError 原文），不静默降级。
- **worker/host 桥**（`wasm-worker.ts` / `worker-host.ts` / `engine-host.ts` / `wasm-pkg.d.ts`）：
  `subscribeRightsOffering` 消息——请求字段先校验（requestId/eventId/shares 非法走
  operationError）；返回经严格 parser `parseQueuedRightsSubscription`
  （`corporate-action-views.ts`：字段穷尽、account 必须 "0"（owner 隔离复核）、股数为正、
  回执与请求 event_id/requested_shares 勾稽）；写命令绑定当前 generation，响应前换档即拒绝
  （防认购落到已切换会话）；宿主不支持时 `EngineHost.subscribeRightsOffering?` 缺失，
  UI 显式提示。
- **UI**（`MarketRuntimeProvider.tsx` → `LocalRefreshViews.tsx` → `CompanyPanel.tsx` →
  `CompanyContractPanel.tsx`）：公司面板「本人配股权益」区新增「配股认购提交」——候选 =
  缴款期内（Open）且本人未排队/未结算且有额度（具名权利或公开配售剩余）的方案
  （`rightsSubscriptionCandidates`）；上限 = 具名权利优先、其次公开剩余
  （`rightsSubscriptionMaxShares`，两者皆无显式 null 不填零）；数量输入校验
  （`rightsSubscriptionInputValid`，规范正整数十进制字符串且 ≤ 上限；engine 仍是权威）；
  提交后受理回执（事件/股数/提交日 + 「当日日终划扣」说明）与拒绝错误显式展示，成功后
  即时刷新权益表（认购进度列反映排队事实）；会话/公司/刷新键变化重置表单。
- **显式后续（不属本批）**：Server（远程）与 Desktop（Tauri）宿主的同款认购命令桥接；
  WASM bindings 重建前旧产物缺该导出时 worker 显式报「请重建 bindings」（既有降级口径）。

## G2 送转公开披露链路

现状（审计 R3）：`AnnouncementContent` 无送转变体、disclosures 不接收
`stock_distributions`——`announce` 只推进状态机，不等于公开发布；玩家公开查询与
NPC 本人获知均不通（拆股 `ShareSplit` 公告已由 S1 接通，本批补送转 `StockDistribution`）。

- **engine**：`AnnouncementContent::StockDistribution(StockDistributionEventPlan)` 新变体
  （载荷 = 方案本体，与 `ShareSplit` 同构，无 `{ plan }` 包装——区别于配股/回购的结构体
  包装）；`public_view.rs::validate_announcement_content` 新分支（`plan.validate()` +
  发行人/公告日与公告头一致，恢复边界共用）；`disclosures.rs::SimpleDayEndDisclosureCtx`
  新增 `stock_distributions` 字段并在 `run_simple_day_end` 与拆股循环并列发布（同日 18:00
  相位、状态门 Announced + 当日到期 → 恰好一次）；`session.rs` 日结调用点传参。
- **NPC 本人获知**：复用既有公开获知机制（`deliver_public_information` 按 attention /
  Immediate cadence 获知公告事实；送转公告不施加 Shock／信用违约信念成因），与现金分红
  公告批同语义；披露前不可见由 `as_of` 前视拒绝（`EarlyRead`）与
  `announcements_for_company` 过滤保证。
- **Web 存档 parser**（公开库投影）：`corporate-actions.ts` 抽出权威方案解析
  `parseStockDistributionEventPlanValue`（账簿 plan 与公告共用，拆股 `parseShareSplitEventPlanValue`
  先例）；`reports.ts` 公告分支与 `AnnouncementContent` 类型联合加 `StockDistribution`、
  `parseAnnouncement` 分发同步。**无新 ts-rs 类型**（`AnnouncementContent` 非 ts-rs 导出，
  与既有公告 parser 手写口径一致），typegen 176 项与基线一致零漂移。
- **trading-rules**：「送转实际登记」节移除「公开 typed 公告尚未接通」边界，登记新口径；
  「配股／增发实际执行」节登记玩家认购宿主入口与 Server/Desktop 后续。

## 红绿证据（`.tmp/company-system/subs-disclosure/`，本 worktree）

| 阶段 | 红 | 绿 |
| --- | --- | --- |
| 送转公告 session 全链路（公告日前不可见→公告日 18:00 发布→NPC 即时获知→成因不变→EarlyRead→恰好一次→restore 深等） | `g2-engine-compile-red.log`（变体不存在编译红）、`g2-engine-behavioral-red.log`（只加变体不接线：`.expect("已批准送转必须在其计划公告日的 18:00 相位公开发布")` 失败） | `g2-engine-green.log`（5/5） |
| Web 公告 parser（正例 + 发行人/公告日/比例/日期顺序负例） | `g2-web-parser-red.log` | `g2-web-parser-green.log`（corporate-actions-schema 32/32） |
| 认购受理回执 parser（owner 隔离/正数/字段穷尽负例） | `g1-receipt-parser-red.log`（临时禁用导出复现缺导出红） | `g1-receipt-parser-green.log`（3/3） |
| worker 认购消息字段校验（requestId/eventId/shares） | `g1-worker-validation-red.log`（临时禁用 case 复现「未知 Worker 消息」红） | `g1-worker-validation.log`（wasm-worker-failure 13/13） |
| 面板认购入口（候选/上限/输入校验纯函数 + 不支持显式提示 + 全支持不误报） | `g1-panel-red.log`（临时禁用三个导出：3 败） | `g1-panel-green.log`（11/11；既有「不误报不支持」用例随新可选支持位补齐 fixture） |
| 引擎认购契约（玩家现金不足显式拒绝不入队 + 受理回执恰一条 + 重复提交拒绝） | 特征化用例（M 批既有行为，不宣称红） | `g1-rights-tests-green.log`（rights session 14/14） |

## 验证范围

只做定向短单测、独立复核与编译／类型检查，未运行完整回归（按批次纪律）。普通测试
单命令/case 10000ms（`run-with-deadline.mjs`），编译/长验收 300000ms
（`run-long-validation.mjs`）；engine 组并发执行（直接调用已编译 test binary，18 组同时）。

| 验证 | 结果 | 证据 |
| --- | --- | --- |
| engine 受影响 18 组并发（company_simple_session 37、rights 14、share_split 8、corporate_actions 19、simple_preferences 16、notices 11、cash_dividend 35、cash_dividend_tax/dividend_tax 41、stock_distribution 13、share_registry 28、ex_reference_price 24、rights_offering 11、issuer_repurchase 3、company_mechanism 3、company::simple 122、information::source_tests 10、share_split 模块 6） | 全绿 | `groups/*.log`、`groups-all.txt` |
| `session::persistence`（31 过/13 败） | 13 败与 pristine HEAD 基线**逐项一致**（本 worktree 还原 diff 后复跑对照，失败集合 diff 为空） | `groups/sessionpersistence{,-BASELINE}.log` |
| 追加公告消费面组：`information::` 18/18、`session::intraday_disclosures` 1/1 全绿；`session::hash_contract_tests`（10 过/1 败）与 `session::decision_chain`（71 过/10 败）失败测试名与 panic 位置与 pristine HEAD 基线**逐项一致**（仅线程 pid 不同），零新增 | 一致 | `groups/session{hash_contract_tests,decision_chain}{,-BASELINE2}.log` |
| `company_operations` 集成测试（52 过/2 败：income_tax 有效期、payment_risks 无效金额） | 2 败与 pristine HEAD 基线逐项一致，零新增 | `company-operations-test{,-BASELINE}.log` |
| Web 全量 `run-web-tests.mjs`（8 shard 并发） | 7 个失败（Worker×3、WASM parser×3、decimal account map ×1）与 pristine HEAD 全量基线**逐项一致**；另 Tauri 更正命令用例在本批全量出现 1 次失败，但该用例在 pristine HEAD **单文件运行同样失败**（S2 批已登记的既有脆弱用例，分片布局敏感），非本批新增 | `web-full.log`、`web-full-BASELINE.log`、`tauri-transport-{mine,baseline}.log` |
| ts-rs typegen + `check-generated-types` | 176 项导出（与 F 批基线同数，无新 ts-rs 类型）、生成物零漂移 | `typegen.log`、`typegen-check.log` |
| `cargo check --workspace --all-targets --exclude stock-market-game` | 0 error | `workspace-check.log` |
| `cargo check -p web-wasm`（host）＋ `cargo +nightly check -p web-wasm --target wasm32-unknown-unknown`（仓库 wasm 配置需 nightly） | 通过 | `wasm32-check.log` |
| Web `tsc -b --force` | 0 错误 | `g1-tsc.log` |
| oxlint 本批 14 个触碰文件 | 0 警告 | `oxlint.log` |

## 已知边界与登记

- **WASM bindings 重建**：`subscribe_rights_offering` 导出需重建 wasm 产物后 worker 才可
  调用；旧产物运行时经 `WasmTransportExtensions` 可选探针显式报「请重建 bindings」，
  不静默。本批验证到 wasm32 目标编译通过，未执行完整 Web release 构建（既有环境前置，
  与 S2 批口径一致）。
- **wasm crate 内导出的 owner 过滤直接测试**：与 S2 批登记的既有共享缺口同类
  （`s2-wrapup-review.md`），本批以 engine 契约特征化用例 + worker/parser 测试覆盖语义，
  wasm 层留待该缺口统一补齐。
- **Server/Desktop 认购命令桥**：显式后续（Web 优先既定），已在 trading-rules 与
  current-handoff 登记。
- UI 交互（真实浏览器点击旅程）未实测：本批 UI 验证为 SSR 静态态 + 纯函数行为测试
  （仓库面板测试既定口径）；真实浏览器旅程受既有 WASM step panic 环境性问题阻断
  （S1×S2 集成批已登记），不冒充已验收。

## 独立复核门禁（大 A 语义）

已请求未实施本批的 subagent 独立审查完整 diff（语义依据、最小范围、边界测试与跨层
语义漂移）；复核结论与处置回填于本节。
