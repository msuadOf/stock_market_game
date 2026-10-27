# 真实 Rust wire 消费追加批独立复核

复核日期：2026-10-28。复核者未实施本批产品或测试改动；工作树为 `/data1/baiyifan/workplace/stock_market_game/.worktree/company-catalog-bc`，HEAD 为 `e2b831fd04bc8b7e6573e9eef4656756075b64b4`。

**结论：本批真实 native Rust wire 的 TypeScript host 消费与 Connected 层验证通过独立复核，未发现新增阻断缺陷。** 原有 14 文件的 SOURCE_ONLY 历史结论不被改写；本轮增加的是有来源绑定的 native JSON 消费证据，不代表 WASM JavaScript export、browser、生产 Remote membership 广播、完整 F5、F6 或真实投资者结算已验收。

## 完整范围与源码绑定

已核 `c10cf187..工作树` 的完整 F5 产品 diff，并重点审查 `e2b831fd..工作树` 的追加内容：两份测试修改、`apps/web/src/host/fixtures/company-read/` 四份 JSON 与中文 README、主题 README 追加及 `real-wire-v1/` 证据。此前 14 文件中仅 `use-company-query.test.ts`、`company-read-hosts.test.ts` 的字节改变，其余 12 文件与已独审通过的 issuer 修复清单一致，生产实现没有变化。

逐项核对 [`real-wire-v1/change-manifest.json`](real-wire-v1/change-manifest.json)：68 文件、212308 字节的路径、大小和 SHA-256 全部一致。该清单不包含自身及后产生的独审报告/日志。最终 [`final-checks-v2/receipt.json`](real-wire-v1/final-checks-v2/receipt.json) 绑定的 18 项源码/fixture 指纹与当前实际文件全匹配。没有操作主树、其他工作主题或产品代码，没有运行 Cargo。

## fixture 来源与执行证据

四个产品 fixture 均与 `company-query-host-2026-10-28/four-host-green-02` 原件及本主题保存副本逐字节相同：

| fixture | 字节 | SHA-256 |
|---|---:|---|
| `wire-engine.json` | 5083 | `241c0007905ad4e3f023c885980b618a79a20fd593dd900b3614bc717462b7da` |
| `wire-server.json` | 4115 | `399e7a0831069d7602accf6fa25b8c3ad9ea9bf1d18cd45613345e8277b40c1a` |
| `wire-tauri.json` | 4111 | `8939c269d5cbe7f41bd8e35b1a28cf5832ebe2dd8bf6d06822af640ff93b2a13` |
| `wire-tauri-failures.json` | 292 | `a63eac0b5f3f0b68672d3b8419b9c9520cab0db9f9b859c4f6c3e45205f0bfa0` |

独立读取原 stdout 与本地精确副本，核对各自 SHA，并按实际 `COMPANY_READ_FIXTURE`、`COMPANY_READ_SERVER_FIXTURE`、`COMPANY_READ_TAURI_FIXTURE`、`COMPANY_READ_TAURI_FAILURE` 标记重取 JSON：四份分别有 6/6/6/1 条，每条结构和值均对应 fixture；engine 独立日志集合不附会为跨日志顺序或同一局状态。

`rust-actual-wait-cleanup.json`、`rust-artifact-binding.json`、`rust-batch-receipt.json` 与 Rust owner 原件字节一致。原 batch 的十八项 exact test 记录包含真实 stdout、child status、unit SHA 与 exit 0；actual wait 为 exit 0、18/18、19.355659 秒，1001 项源码未变，记录的 owned identity/argv 残留为空且 scratch 已清理。另独立核对 Rust 原 `source-freeze`、before、after 的 1001 项清单相同，当前 owner 树逐项 SHA 也匹配；冻结 `01d0ac7eb28ef11a0eb92a763e446bfc1cc6def4` 至交付 `aa016fbd435d39ab37b3d9c7d6cce57d3b3d2131` 的 `apps/`、`packages/` 无实现差异。

本轮复用已保留的 Rust 执行与独审证据，没有重新构建或执行 Rust unit，也未重新计算 RAM 中大型 unit 的完整二进制 hash。不能把本报告视为重新执行四包 native 验收。

## 三道门禁

1. **A 股概念与数值口径**：当前权益、代扣回执、未划收税额与历史划收回执继续区分。股数为股，`Money` 为规范分字符串，`AccountingAmount` 为两位元字符串，比例为 bp；没有浮点改写或 UI 推导结算。`cash_settlement=false`、五类 `ready=false`、尚未接线与简税说明均保留。沿用此前 `docs/trading-rules.md` 的公司只读、个人差别化税务与 FlatWithholding 简化边界，未新增交易制度或参数，不声称本轮重新联网核验官方规则。
2. **必要性与最小范围**：新增测试补足此前 mock 数据不能证明真实 Rust serde 形状可被严格 parser 消费的缺口；没有新增生产分支、依赖、fallback 或写命令。四 fixture 按 SHA 固定，测试不重写原件。engine 中非空税账视图的 outstanding 实为零，不能把“有一条税账记录”说成存在非零欠税；Flat receipt 和权益也是领域隔离输入，不冒充同局真实派息、税款划收或认购结算。
3. **身份、代际和跨层一致性**：真实结果 owner 均为规范字符串 `"0"`，Server/Tauri generation 为 `"1"`；公司代码通过 actual catalog 得到 `CompanyId=0`，权益 issuer/stock 仍经上一轮修复后的对应关系校验。响应 kind 与请求一致，现有错 kind、其他账户、dispose、同代 baseline 重装和 Connected membership 失效测试均保留。真实原件只提供 owner 0，其他 owner 的负控与大整数边界由既有隔离测试承担，不能将本批合法空列表扩称为跨 owner 差异正控。

## 是否真正穿过 host 与 Connected 层

- 三个新增成功 case 分别调用实际 `createWorkerHost`、`createTauriHost`、`createRemoteHost` 的六个查询，再经过既有 `createCompanyReadQueries` 与严格领域 parser；每类六次，共 18 次实际 host 读取。Worker 受控消息端返回 native domain fixture；Remote 使用受控 fetch/WebSocket；Tauri 使用 mockIPC。没有把直接调用 parser 的结果冒称 host 消费。
- 新增 Tauri 失败 case 注入实际 IPC 序列化拒绝对象，实际 `queryCompanyPeriodExplanation` 拒绝为 `Error`，断言保留 `BusinessCondition`、`2030-01-31` 与“尚未结算”，且没有 `[object Object]`。
- Connected 新旅程挂载真实 React、Redux `Provider`、`MarketRuntimeProvider` 和 `ConnectedCompanyPanel`，注入实际 `createRemoteHost`；由该 host 经受控 transport 消费同一 server session 的六种 JSON。断言 Bearer fixture 身份、请求不带 account、实际公司 ID、总股本、元单位利润、简税说明、真实空税账、手动输入期间后的解释以及无写按钮/结清/查询失败文字。baseline 与 membership 显式受控，因此该旅程不证明生产 membership 广播或真实网络连接。
- 首三次 App 消费失败已核：清理路由遗漏、启动路由遗漏、MemoryDOM 不支持 select options；均属于 fixture 基础设施失败，不计为业务 TDD 红灯。最终改用已存在的 `stockContext` 个股入口，业务显示、六 query、公司 ID 与错误否定断言仍保留，没有修改生产或删除业务断言。

## 独立运行与既有静态结果

独立执行两份最终测试文件，**36/36 通过（host 19、UI 17）**，exit 0，Node 报告 1750.109326ms。原始日志见 [`node.stdout`](real-wire-v1/independent-review/node.stdout)、[`node.stderr`](real-wire-v1/independent-review/node.stderr)。命令如下：

```sh
/data1/baiyifan/workplace/stock_market_game/.tmp/node24-env/node /data1/baiyifan/workplace/stock_market_game/.worktree/main-merge-2026-10-25/scripts/run-with-deadline.mjs 10000 -- /data1/baiyifan/workplace/stock_market_game/.tmp/node24-env/node --test --test-timeout=10000 --test-concurrency=2 apps/web/src/host/company-read-hosts.test.ts apps/web/src/components/company/use-company-query.test.ts
```

已核首轮四个 strict App root shard exit 0（均低于 7.5 秒）、host 19/UI 17 绿及两条新增 lint warning 原件；最终 props 变量窄修后只重跑受影响 UI、所在 strict shard 与 lint，三项均 exit 0。四分片合计 764 entries/761 唯一路径，公共 ambient declaration 在各 program 重复，不冒称额外根文件。App 原三条 hooks warning 仍是既有有限 lint 边界，不能汇报全仓 lint 全绿。

本批可以提交为真实 native wire 的 TS 消费验证。WASM JavaScript/export、browser、完整 AppShell 生命周期与生产 Remote membership 接线、桌面打包及真实公司结算仍须独立验收；本报告没有将其列为本次测试追加应修改的生产范围。

提交前文案复核：`real-wire-v1/README.md` 仅将“未划收税额”精确澄清为非空税账记录且当前 `outstanding` 为零，明确不能称为非零欠税；逆向替回原句可精确还原前审 README SHA。新 README 为 3558 字节、SHA-256 `62b6828bac056937624003ec5546cd45514117d1f8af4f1c8ad6f4d5833f697b`，清单同步该项与总字节数。再次核对 68 项大小和 SHA 全部匹配；本次没有产品/fixture变化，不重跑测试，通过结论及限制不变。
