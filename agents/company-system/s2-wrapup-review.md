# S2 收尾批次独立复核记录（2026-10-07）

复核人：非实施 subagent（经 codex 通道，只读复核主树未提交 diff；codex companion
通道故障后由备用通道完成，两轮结论合并登记于此）。实施人未参与本记录的裁定。

## 第一轮（完整 diff）

**major：** 无。

**确认项**

- 事件 id 派生与引擎逐字同构（`session-market:{stock}:{account}:{day}`＝
  `packages/engine/src/session/corporate_actions.rs:1672`；`{event_id}:{account}`＝
  同文件 `nontrading_tax_day_event_id` rs:215-217）；账户匹配条件与引擎一致
  （changes 含 Account 条目即要求覆盖，不要求 change≠0）；错误文案一致。
- `owner_rejected_rights_subscriptions` 确按 `AccountId(0)` 过滤；偏好数值域三层
  （engine validate／`parseSimpleCompanyPreferences`／UI 表单）逐项一致。
- `packages/engine` 零改动；改动均落在 S2 声明范围。
- checklist 新勾 4 项证据文件全部存在；勾选计数 19/20 与摘要一致。

**minor（已全部修复并复测）**

1. Web 覆盖勾稽对 IssuerRepurchaseCancellation 用隐式 else 跳过，非引擎式穷尽
   match → 已改为三分支穷尽＋`const exhaustive: never = scope` 编译期强制与
   运行期显式抛错（`apps/web/src/save/schema/corporate-actions.ts`）。
2. 缺回购注销回执、零变动账户条目两分支用例 → 已补
   `corporate-actions-schema.test.ts`「回购注销回执显式跳过、零变动账户条目仍
   要求覆盖」（26/26）。
3. 偏好台账面板未注明数据截止语义 → `PreferenceRejectionsPanel` 头部固定注明
   「读取最近一次已完成自然日日终的存档状态」。

## 第二轮（复核上述修复后追加发现）

**major：** 无。

**minor**

- M1 `App.tsx` 把可抛错的 `changeCompanyPreferencesDraft` 放进 React state
  updater（渲染期执行、异常绕过外层 try/catch）→ 已改为与姊妹处理同模式：在
  updater 外先计算（两处：局内新局表单与启动创建屏）。
- M2 偏好编辑置 origin=custom 后，切换结算周期的报错文案把「手改 JSON」与
  「表单设偏好」混同 → `changeSettlementCycleDraft` 错误文案已区分两种来源
  （origin=custom 本身必要：防 seed 预设重建丢偏好；`createPriceAnchoredCompanyConfig`
  经 structuredClone 保留偏好已由复核人验证并补回归测试）。
- M3 两个新 wasm 查询缺自动化契约测试 → 部分修复：worker 消息字段校验契约已
  钉住（`wasm-worker-failure.test.ts` 新增无效 requestId／公司身份用例）；
  wasm crate 内 owner 过滤/存档读取的直接测试与既有 `owner_dividend_tax_status`
  系导出共享同一缺口（构造完整会话成本高），登记为后续测试批次遗留。

**note（处置）**

- N1 公司身份 64 长度校验三层单位不一致（Rust 字节 vs JS UTF-16 码元）→ 已统一
  为 `encode_utf16().count()`。
- N2 勾稽循环 JSON.stringify 比较 holder 与上方 `"Account" in holder` 风格不一致
  （单键规范形状下正确）→ 保留不改（改写引入不一致风险大于风格收益）。
- N3 `company_preference_rejections` 每查克隆整个日终 SaveSlot → 已知权衡，日终
  频次下量级可接受；如成瓶颈由后续引擎只读 accessor 批次处理（本批不碰
  `packages/engine`）。
- N4 偏好编辑重排 JSON 格式化（内容全保留）→ 接受。
- N5 缺「偏好编辑后改 seed/重新生成不丢偏好」回归测试 → 已补
  `seed-draft.test.ts`（changeSeedDraft 与 regenerateSeedDraft 两路径均断言偏好保留）。
- N6 复核环境 Rust 工具链冲突未能独立 cargo check → 实施侧 `cargo check -p web-wasm`
  通过（`.tmp/company-system/s2-wrapup/cargo-check-web-wasm.log`）；复核人实跑 7 个
  相关 TS 测试文件 51/51。

## 复测证据

`.tmp/company-system/s2-wrapup/`：`affected-groups-final.log`（提交后状态复跑
18 文件 156/156，含 worker 契约用例）、`tsc-final.log`（0 错误）、
`cargo-check-web-wasm.log`（通过）、`panels-red/green.log`、`parser-parity-red.log`、
`web-baseline-before.log`／`web-final*.log`／`preexisting-failing-files.log`
（存量失败枚举与零新增对照）。实施人在两轮修复落盘后对全部改动逐行复核并复跑
上述验证（复核人越权直接改码的部分已由实施人接管核验，结论一致）。
