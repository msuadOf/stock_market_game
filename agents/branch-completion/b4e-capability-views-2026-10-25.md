# B4e 共同能力与本人配股只读查询

## 来源与范围

- 基线 `35ac58a1`，独立 worktree `.worktree/b4e-capability-views`，分支 `feat/b4e-capability-views`。
- 对应 `sol-readonly-inventory-2026-10-25.md` 可执行任务表「B4e 共同能力/本人配股查询」。旧 `18725d07:session/company_contract_views.rs` 仅提供投影结构素材，没有覆盖旧 Session 或恢复动账链。
- 新增 `company_capabilities`、`owner_rights_offerings` 两个只读 API；公共本人配股类型置于 `session/owner_rights_views.rs`，从 `session` 导出。`CompanyId` 保持当前数值身份，`StockCode` 保持证券代码字符串，Money 为十进制分字符串，股数为 canonical u64 字符串。
- 保留当前 `corporate_actions() -> Option`；四个税务/期间 query 由并行切片负责。审批、认购、付款、提案、host、UI 和 ts-rs 导出均不在本批。无新依赖、存档字段、默认补值或迁移。

## 语义

- `CompanyFacts` 当前实际结算限制原样投影：`cash_settlement=false`；五类 `action_readiness` 显式阻断当前命令/实际投资者结算未接线，配股/回购开关另有 blocker。没有照搬旧静态可受理声明或 `.ok()`/`unwrap_or(false)` 吞错链。
- 公司能力的 `owner_rights` 同时按本人账户和发行人过滤；账户查询仅含本人具名权益、本人排队和本人已入账认购事实，公开额度为全方案共享事实。公开额度按 requested 而非 paid 计占用；合计溢出和透支显式失败。
- 仅投影已有领域状态机；现金分红 Paid、送转 Credited、拆股/配股 Settled、回购 Cancelled 过滤。回购 Completed 保留，表示等待股份注销或零成交终态事实，未虚构注销。
- 缴款窗口两端包含在 Open 内；权证未派发或无公开额度是 None，公开额度耗尽是 Some("0")。查询不触发结算。
- 依据沿用 ADR-0035/0037/0039、领域 `rights_offering.rs` 与 `agents/company-system/remaining-corporate-rules-research.md` 的官方条文核验记录。2026-10-25 再访问中国结算深圳指南官方直链，直连与原留存代理均返回约 988 字节 HTML 防护页，不能称本轮重新核过完整 PDF；本批没有新订交易制度或把深圳日程冒充上海已证规则。HTML 原响应保存在根 `.tmp/company-capability-views/official-guideline-response.html`。

## TDD 与验证

- 先加入公共类型、API/helper 显式未实现 stub 与测试。首次编译暴露夹具导入错误，修正后才取得有效 RED：5 case 中4因「B4e 查询尚未实现」失败、1错误边界通过，二进制执行0.92秒；见根 `.tmp/company-capability-views/red-tests.log`。
- 实现后初始5 case 绿0.90秒。独立预审要求补齐五类状态、issuer过滤、精确拒绝原因及合法 Session 名册；补测10 case 首轮8绿2红，真实暴露初始名册 R-1 与 R 登记时点不符，随后补真实空变动 PublicMarket 日结，不弱化断言。
- build 单独分类为必要长构建：`CARGO_BUILD_JOBS=16`、`cargo test -p engine --lib --no-run -j16`，独立根 `.tmp/company-capability-views/target`；由 `runBoundedCommand` 进程外 supervisor 总300000ms deadline，编译与测试分开记录。实际观察 rustc 前端单进程，后续 codegen 多线程；未把配置jobs当实际活跃核数。
- 普通测试用已编译二进制、`--test-threads=8`，整命令进程树10000ms deadline，覆盖所有 case 的更紧上限。未跑完整回归、host/Web、浏览器或完整长公司结算验收。
- 最终结果与独立终裁待追加；本记录当前不代表门禁完成。

## 冻结候选验证

- 最终独立长构建 `freeze-build.log`：36.35秒成功，已有573条 warning（本批不顺带清理）。
- 最终定向 `freeze-tests.log`：10 case 全绿，1.54秒，`--test-threads=8`，进程树 deadline10000ms；`git diff --check` 通过。
- binary：根 `.tmp/company-capability-views/target/debug/deps/engine-f7ff6b8af6493b4b`，SHA-256 `dd9e8aef5c49f991f641c20493fe1bc63fa4d6809517012ef5ba9eccc4b26987`。
- 按仓库 `collectFullRegressionSourceFingerprint` 对当前源码（含必要未跟踪源码）取证，完整文件清单位于根 `.tmp/company-capability-views/source-fingerprint.json`，digest `2046393dba4f4a3ce9749ebaf683f1c92b7e7c785c4fbecd1f860438b32c94d4`。未把此定向验证扩称全量回归通过。
- wrapper 存档前后字节一致测试使用实际 Session 的100000股名册，先按真实 PublicMarket 空净变动完成 R 日名册后登记；纯领域状态机 fixture 独立标识，不声称触发完整日终结算或存档恢复。
