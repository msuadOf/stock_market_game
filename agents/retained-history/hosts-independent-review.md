# 永久量价历史宿主接线独立复核

## 结论

跨宿主请求遵守 ADR-0034：公开历史查询不在请求中接受账户 ID；Server HTTP 路由先授权，再按本人 membership 记账，未入场访客走纯公开读取；远程、Tauri、Worker 查询都绑定当前 generation，响应再做严格结构归一化。分钟数量/成交笔数使用规范 u64 十进制字符串，成交额使用 u128 字符串，日期校验为有效 ISO 自然日，分页日期升序连续且检查 cursor/page_size 和提前截断。

初审发现的编译类型问题已按下文复核修复；当前未发现遗留阻塞问题。独立对照 Server 原始失败和修复后短测日志，确认必填 nullable 断言没有被弱化。

## 复核结果

- 原始 `.tmp/retained-history/hosts-server-short.log` 在 `auth_tests.rs:188` 因实际响应 `200`、预期 `400` 而失败；该断言位于缺失 `after` 请求之后。它之前的未授权、授权公开读取、访客公开读取、stale generation、账户注入断言均已依次通过。
- Engine `MarketHistoryRequest.after` 现使用 `#[serde(deserialize_with = "required_nullable")]`；该 helper 接受显式 `null` 或日期，缺字段时由 Serde 拒绝，不使用默认值或兼容补齐。Engine 短测验证显式 null 成功、缺失字段失败；Server 测试仍保留 `remove("after")` 与 400 断言，未删除、放宽或改写。
- 修复后 `.tmp/retained-history/hosts-server-host45-short.log` 显示同一 `routes::auth_tests::retained_market_history_auth_generation_and_public_nonmember_query` 1/1 通过，耗时 0.19s。通过结果覆盖原先失败断言，因此红绿证据闭合。
- 实现者报告 host45 五个 Rust crate compile 成功；本复核未运行 Cargo。该实际测试日志与编译报告不替代 root 后续统一验收。

## 跨层核对

- Server：新增 `/api/market-history` 使用 `deny_unknown_fields` 的请求体，并验证授权、generation；成员读取走 `query_market_history_for(account, ...)`，未成员走无记账 `market_history_page`。独立 HTTP 短测覆盖未授权、访客可读、stale generation、账户注入与缺字段，见 `apps/server/src/routes/auth_tests.rs:163`。
- Rust 编译复核：`apps/server/src/actor.rs:2548` 的成员/访客分支都返回 `SessionError`，因此其错误适配已显式转换为 `SendCommandError::Rejected(error.to_string())`；membership 分支仍单独用 `MembershipError` 映射。类型边界现与两个 Engine 查询 API 一致，保留上下文错误且不改权限/记账行为。
- Web：`market-history.ts` 严格检查请求字段、代码、日期、正 u32 page size、bar OHLC/成交量/成交笔数/成交额、日期顺序、可用性与游标。`remote-host.ts:543` 校验 generation 和当前身份未切换；`tauri-host.ts:433` 校验 generation；Worker 请求也携带 generation 且解析返回页。
- 最终页 parser 还要求覆盖请求游标之后至 `page_size` 的连续自然日；剩余日期未耗尽时必须返回完整页和精确 `next_cursor`，范围耗尽时必须不带 cursor，拒绝遗漏日期、空页或提前截断。Remote、Tauri、Worker 短测 fixture 已用连续 `NotEnded` 日期。
- 宿主短测：Remote 覆盖未入场读取、请求无账户字段及过期 generation（`apps/web/src/host/remote-host.test.ts:19`）；Tauri 覆盖命令参数（`apps/web/src/host/tauri-host.test.ts:48`）；Worker 覆盖请求 generation、无账户字段及丢弃旧 generation（`apps/web/src/host/worker-host.test.ts:21`）；wire 单测覆盖大整数不转 Number、非法输入和页错配（`apps/web/src/host/market-history.test.ts`）。
- A 股语义/范围：本批只暴露既有分钟成交事实和日 K，不生成逐笔、账户、对手方或成交外推；分钟阶段及 OHLCV/金额单位与 core-contract 一致。新增能力是 ADR-0034 所需的宿主/API/FFI 接线，范围合理。无需据此修改交易制度文档。

## 验收状态

本 reviewer 未执行测试或 Cargo。实现者报告 Web 相关 6 个短测通过（0.42s），root 报告 TypeScript 检查通过（6.99s），并报告 host45 五 crate compile 成功；上述结果按报告归属记录。本人亲读 Server 原始失败日志与 host45 最终通过日志，确认测试断言未弱化且指定权限/nullable 边界通过。完整回归仍由 root 统一验收。
