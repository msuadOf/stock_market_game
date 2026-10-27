# App F5 只读接线独立复核

复核日期：2026-10-28。复核者未实施本批产品改动；只读候选树 `/data1/baiyifan/workplace/stock_market_game/.worktree/company-catalog-bc`，基线 `c10cf187c9f498dbabde9defc33042147a79aefe`。

## 范围与结论

完整审查本次 App 只读 dirty diff：`App.tsx`、`app/LocalRefreshViews.tsx`、`components/company/CompanyPanel.tsx`、`components/company/use-company-query.test.ts`、`host/engine-host.ts`，以及新增 `components/company/CompanyReadPanel.tsx`。实际为 **5 个既有文件修改 + 1 个新增文件，共 6 个产品文件**。没有审查其他工作树、Rust producer、Cargo 验证或随后尚未加入的 TypeScript transport 实现。

本范围未发现可给出“具体输入 → 错误结果”的新增阻断缺陷。结论仅为该 App 只读消费片通过独立复核，不代表真实 host、Remote membership、browser 或公司行为结算已经验收闭合。

## 三道门禁

1. **大 A 语义与依据**：改动只把统一 `EngineHost` 的六类可选只读查询接到既有 `CompanyContractPanel`、`DividendTaxPanel`；没有引入提交提案、批准、认购、扣款或划收命令，没有更改 `cash_settlement`、`action_readiness` 或已有 `ready` 判定。公司代码来自当前 actual catalog，并用现有 `CatalogResolver` 得到 `CompanyId`，没有按数组位置猜身份。金额、股数、bp 和分数税额沿用既有投影，不新增浮点转换。沿用 `docs/trading-rules.md` 的 B4e 只读边界，以及分红与送转候选节登记的财税〔2015〕101号、财税〔2012〕85号和中国结算指南依据及其适用范围；本批不改交易制度，没有重新联网核验官方原件，也不将本次消费层审查扩称官方规则全面重验。
2. **必要性与最小范围**：App 与 mobile 仅传入当前 host，Connected 层构造 owner/refreshKey，新增薄组件集中转接查询，既有面板负责展示与错误。六个查询都属于本次公司能力、期间解释、本人权益与本人税务只读范围；没有新增依赖、持久化、第二套数据源或写入能力。
3. **边界与跨层一致性**：`host`、`owner`、公司代码变更使 memoized callback 改变；snapshot generation、company generation、owner、自然日变更使 refreshKey 改变。既有 `useCompanyQuery` 按 callback/key 同步隐藏旧数据，并以 ticket 和 effect cleanup 拒绝迟到结果。owner 为 null 会卸载本人子面板，host 为 null 显式说明尚未就绪；缺失方法由子面板显式说明不支持，查询失败交既有错误 UI。本人查询不接受 UI 传入账户参数，账户授权仍须由 host/producer 保证。

## 原始证据与独立复跑

- `red-v1` 两条失败均为 MemoryDOM/select 基础设施 `TypeError`，不算业务红灯。
- `red-v2/read-panel.stdout` 的真实失败是缺少“公司能力面与当前方案”；`red-v2/owner-removal.stdout` 的真实失败是缺少退出身份提示。两条为业务断言失败，不是工具启动失败。
- 已核 `green-v1`：React 消费测试 15/15，SSR 4/4。
- 本人独立复跑日志：[`independent-review-v1/node.stdout`](independent-review-v1/node.stdout)、[`independent-review-v1/node.stderr`](independent-review-v1/node.stderr)。命令 exit 0，19/19 通过，Node 报告 wall 3355.815575ms；并发参数为 `--test-concurrency=2`，普通命令与 case 上限均 10000ms。

```sh
node /data1/baiyifan/workplace/stock_market_game/scripts/run-with-deadline.mjs 10000 -- node --test --test-timeout=10000 --test-concurrency=2 apps/web/src/components/company/use-company-query.test.ts apps/web/src/components/company/CompanyContractPanel.test.ts apps/web/src/components/company/DividendTaxPanel.test.ts
```

没有运行 Cargo、261/290 长批、完整回归或真实 browser。

## 尚未由本片证据证明的边界

两条新增用例直接挂载 `CompanyPanel` 并注入 mock `EngineHost` 与 `CompanyReadContext`，不是实际 `App` 或 `ConnectedCompanyPanel` 测试。因此，它们证明 actual company code 转接与 owner 移除时清除旧组件事实，不能证明 Remote membership 在生产更新中已写入 Redux，也不能证明本人查询返回的真实 `response.owner` 已与当前宿主身份核对。

父任务已明确这两个生产接线点正在独立处理，本报告不把它们冒称已关闭，也不将未落入本次 App diff 的已知工作重复列为新增缺陷。最终 transport/membership 片必须另核具体场景：本人 A 已显示税表 → membership 失效或切本人 B → 旧事实立即清除、旧成功/旧错误失效、B 的新请求仅可显示 B；以及 host、generation、自然日和实际公司映射切换。当前 hook/面板通用测试覆盖 callback/key 失效机制，但不替代这条生产接线证据。

## 本次复核源码指纹

以下 SHA-256 绑定本报告所读 App 片；后续 transport 或修复改变文件时应增量复核。

```text
066df096e9751e87bcf0496e481e7999d632ff03938acceddf71567c9a7fd780  apps/web/src/App.tsx
2cf12126f62b6e05b72c21fea81a1232847f9cfae431e0902498fc20a52c7c78  apps/web/src/app/LocalRefreshViews.tsx
3501b3b549a36451965801be666c8bfcb96ad12eb8e41223df323782365dbcce  apps/web/src/components/company/CompanyPanel.tsx
f174c0ddba8afdc359d50145f45e23d495ae65344d84affd14e1aff0051cb34d  apps/web/src/components/company/CompanyReadPanel.tsx
69904c3ab65f737634aeb0cadfe4000b692c1dc3872e30b4b3e8b3eee3a6a10e  apps/web/src/components/company/use-company-query.test.ts
c60edb3210579fadbc975626da34ddcb7ed0301ceb59cd142b055483d90e3ccc  apps/web/src/host/engine-host.ts
```

## 第二轮：完整 TypeScript 查询接线预审

本轮范围扩为 14 个产品文件（10 个既有修改、4 个新增），冻结清单见 [`static-v2/receipt.json`](static-v2/receipt.json)，该 receipt 的 SHA-256 为 `c2cd591c16c91e790275aafe0a1e9a64980dc15ae227b01ea7849fc14422ef71`。已读完整 diff 与全部新增源文件，并对照另树实际 Rust `CompanyReadQuery` / `CompanyReadResult` 的 adjacent `{kind,data}` enum 源码；该源码在审查时仍有注册 stub，绝不据源码对照签发真实 producer 或宿主编验通过。

### 发现 F5-APP-R1：配股事实未核对 issuer 与证券的目录关系（P2，待修复）

- 位置：`apps/web/src/host/company-read.ts:60`。
- 具体输入：当前 actual catalog 为 `600101 -> CompanyId(7)`、`600102 -> CompanyId(8)`；`OwnerRightsOfferings` 成功响应返回合法形状的 `stock="600101", issuer=8`。
- 错误结果：只调用 `stockId` 和 `companyCode` 分别检查存在性，两者均存在，查询成功；`CompanyContractPanel` 把 `600101` 的权益显示为“公司编号 8”，与当前权威目录矛盾。
- 必要修复：使用同一当前 `CatalogResolver`，校验所返 issuer 的 `listed_stock` 与该 stock 的实际 `StockId` 一致；不匹配时明确拒绝。增加存在但互不对应的两个发行人/证券测试，不修改 UI 或领域行为。
- 原始失败证据：[`independent-review-v2/issuer-mismatch.test.mjs`](independent-review-v2/issuer-mismatch.test.mjs)、[`independent-review-v2/issuer-mismatch-v2.stdout`](independent-review-v2/issuer-mismatch-v2.stdout)，真实业务断言为 `Missing expected rejection`。普通测试 outer/case 均 10000ms，并发参数 2，运行 223.628225ms。首次 `issuer-mismatch.stdout` 仅为审查工作 fixture 相对 import 路径错误，不计为业务红灯，原件保留。

本轮结论：**需要修复 F5-APP-R1 后再次独立复核，当前不签完整 14 文件完成。** 第一轮 6 文件 App 薄接线结论不代表该后续新增 transport 已通过。

### 已核对的安全边界与验证

- 六种请求没有 owner 输入；company code 在发送前通过 actual catalog 转成 `CompanyId`，日期在发送前严格校验。响应 envelope、`kind`、数据字段均严格解析。
- Local Worker/Tauri 明确要求 `owner="0"`；Remote 使用已认证请求返回的规范十进制 owner，Flat receipts 与 outstanding 每条 account 必须匹配该 owner，不把 owner 转成 `Number`。Remote 的服务端认证与当前玩家身份仍须真实 Rust route 验证。
- 三个 host 拒绝 dispose 后或同 generation 更换 baseline 后的旧查询结果。Worker listener 在读取当前 handle 前要求 generation 正确。未知 bindings 导出会显式报错。
- Connected 新测试确实挂载真实 Redux `Provider`、`MarketRuntimeProvider` 和 `ConnectedCompanyPanel`，验证 `ready=false` 不查询、`ready=true` 开始查询、membership generation 与 snapshot 不一致后撤销读面及迟到成功。它仍是注入 host 的组件边界验证，不能证明生产 Remote membership 广播已接线。
- Tauri 结构化错误从对象直接 reject 改为复用既有 `describeWasmFailure` 构造带 cause 的 `Error`，保留 query kind 等可读详情。已核 [`tauri-error-red/test.stdout`](tauri-error-red/test.stdout) 的真实失败，以及独立复跑中修复后的绿灯；真实 Rust 返回 reason 仍待原件验证。
- 本人独立复跑 [`independent-review-v2/read.stdout`](independent-review-v2/read.stdout) 为 35/35（decoder 4、三 host 15、UI 16，1925.080139ms）；[`independent-review-v2/listener.stdout`](independent-review-v2/listener.stdout) 为 1/1（323.757269ms）。这 36 项不含随后新增的发行关系负例，后者独立失败如上。复跑使用绝对主树 `scripts/run-with-deadline.mjs 10000`、Node `--test-timeout=10000 --test-concurrency=2`。listener 第一次启动因并行创建证据目录时目录尚不存在，shell 在 Node 启动前退出；目录存在后重跑 exit 0，没有把首次启动失败冒称测试失败。
- `static-v2/receipt.json` 绑定最终 fixture 窄修后的 14 文件，四个 strict TypeScript shard 均 exit 0，耗时 6.60–7.09s；改动范围排除 `App.tsx` 的 lint exit 0；最终 Connected fixture 单项 exit 0。本人的 36 项复跑发生在最终测试 fixture 的 `children` props 窄修附近，最终 fixture 的通过由该独立保存的 static-v2 结果补足。
- `lint-v2` 曾发现本批两个 `react(no-children-prop)` 警告并交回修复；最终 props 变量窄修已读，不能把这次失败说成仅有继承警告。`App.tsx` 的三个既有 hooks 警告由显式相同 oxlint 配置的 HEAD 副本对比确认，仍不宣称全仓 lint 全绿。

本轮没有改变沪深交易规则、结算顺序、税率、能力 readiness 或写命令；新增改动范围必要且保持 query 适配层。未运行 Cargo、完整回归、旧 261/290 批次或 browser；真实 Rust 输出与其查询失败 reason 仍待后续原件验收。

## 第三轮：F5-APP-R1 修复复核（SOURCE_ONLY 通过）

F5-APP-R1 已修复并通过独立复核。当前结论为 **14 文件 TypeScript 只读接线在 SOURCE_ONLY 范围通过，未发现剩余阻断缺陷**；不代表真实 Rust producer、实际宿主编验、browser 或 F5 总成已通过。

- 已逐条核对 [`issuer-green/receipt.json`](issuer-green/receipt.json) 的 14 项实际 SHA-256。相对 `static-v2` 仅 `host/company-read.ts` 与 `host/company-read.test.ts` 两个文件变化，其余 12 文件字节不变。新 receipt 的 SHA-256 为 `1c59128e421e13aed02d1659b742774818f04f74598e5be189ae04c708214783`；独立核对结果见 [`independent-review-v3/manifest-check.json`](independent-review-v3/manifest-check.json)。
- `assertIssuerStock` 用同一 actual catalog 核对 `issuer.listed_stock === StockId`，分别用于完整 `OwnerRightsOfferings` 和当前请求公司的 `CompanyCapabilities.owner_rights`。未知 issuer、无上市证券或证券不匹配都会明确拒绝，没有改 UI、领域执行或过滤掉坏事实后继续显示。已对照既有 `GameSession::company_capabilities` 按 `view.issuer == company` 筛选摘要的源码，新增摘要约束没有扩大该查询范围。
- 正式两条独立负例在 `issuer-red-v2` 中均为真实 `Missing expected rejection`；修后测试各保留 `CompanyId(7)/600101` 正例，避免把全部权益查询都拒绝的伪修复。
- 原独审 probe 源文件及断言未变，直接与正式 decoder 测试一起独立复跑 **7/7 通过**，exit 0，253.539784ms。日志见 [`independent-review-v3/issuer.stdout`](independent-review-v3/issuer.stdout)、[`independent-review-v3/issuer.stderr`](independent-review-v3/issuer.stderr)。使用绝对 `main-merge-2026-10-25/scripts/run-with-deadline.mjs 10000`、Node `--test-timeout=10000 --test-concurrency=2`，未重跑无改动的长批或全量测试。
- 已核 `issuer-green` 原件：正式 decoder 6/6、受影响 strict TypeScript shard exit 0（6.44s）、两文件 lint exit 0；批次 `maxParallel=3`。本次修复没有新增依赖、交易制度或数值单位变化，属于修复当前目录身份一致性的必要最小改动。

后续真实 Rust query 输出及失败 reason 原件到齐时，仍须验证 serialization/decoder 与实际 transport 一致；生产 Remote membership 广播和 browser 闭环不由这里的 mock transport 与组件验证替代。
