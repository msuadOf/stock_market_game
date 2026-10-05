# Server 身份契约测试迁移记录

## 范围

迁移 `apps/server/tests/report_corrections.rs`、`ws.rs`、`deployment_routes.rs`、
`api_contract.rs`、`pause_security.rs` 与 `apps/server/src/routes/auth_tests.rs`。
生产路由、`shared_market_rest.rs` 和 `identities.rs` 不在本次修改范围。

## 契约依据

按 ADR-0030，市场控制能力与本人账户/交易能力分开授权；市场成员由认证主体加入，不能由请求
提供账户 ID。按 ADR-0033，用户名密码与匿名登录产生持久身份，凭据由 `Authorization: Bearer`
传递。`/api/new` 需要真实身份凭据，返回 market context，不返回 `session_token`。

## 迁移内容

- 新增集成测试 fixture：基于测试 `SessionManager` 数据库签发 guest credential，并以 creator
  身份创建共享市场。
- 报告更正、WS、暂停安全测试改为使用 guest bearer credential；WS 与报告更正/暂停测试均读取
  当前 `MarketContext.generation`；报告更正测试另加入第二个市场成员，
  验证有本人账户成员资格不等于市场控制权限。
- `api_contract.rs` 与路由单测创建市场时先通过 `/api/auth/guest` 获取真实 credential，再调用
  `/api/new`；契约辅助字段 `test_credential` 只存在于测试进程内，不代表 HTTP 响应字段。
- 保留现有 malformed body、generation、伪造账户字段与访问控制断言；新建市场响应明确断言没有
  `session_token`。

## 验证与限制

本次未启动 Cargo，按协调要求留待 root 的统一验证批次。未取得先红测试证据，因此本记录不声称
完成 TDD 红绿过程。仓库原测试使用旧 `session_token` 响应契约；该既有契约与新身份 API 不兼容，
应以统一验证批次实际结果为准，不将尚未执行的测试报告为通过。

`GET /api/speed` 是只读当前采样指标查询，当前路由只需 `session_id`，不接受 generation；写入速度
设置与其他 timeline mutation 使用 market context 的 generation。不存在的市场探测用例使用一个有效
guest credential，但该 ID 本身没有可读取的 market context。

## host34 短测证据与修订

root 于 host34 产出 fresh test binary；manifest 为 `.tmp/checklist-wave4/host34-binaries.json`。
逐一 `--list` 确认目标中的精确用例存在后，按每例进程外 10 秒 timeout、最多 8 例并发、
`RAYON_NUM_THREADS=4` 与 `--test-threads=4` 执行。日志保存在
`.tmp/checklist-wave4/server-identity-migration/`。

- fresh `pause_preferences_requires_owner_and_current_canonical_generation`：1 passed，0.20 秒。
- fresh `combined_exposes_identity_routes_and_protects_identity_lookup`：1 passed，0.01 秒。
- fresh `report_correction_failure_keeps_actor_available_for_public_cancel_and_retry`：1 passed，0.84 秒。
- `report_correction_routes_require_owner_generation_and_keep_control_separate`：真实失败表明共享更正 GET 对第二个已加入成员返回 200。路由使用 `authorized_market` 读取共享市场更正；因此用例已修为该成员可以读取，并额外断言其 POST/DELETE 仍为 403。修改后须由新 binary 重验。
- `new_session_returns_200_with_id` 与受同一 setup helper 影响的 API 用例失败，错误明确指出 setup 缺 `report_frequency`。已在 `api_contract.rs`、`ws.rs` 与 `routes/auth_tests.rs` 的 JSON setup fixture 补 `Quarterly`、`company_operations: null`、`groups: []`。修改后须由新 binary 重验；旧 binary 不可作为修复后的绿证据。

本次测试源在上述 red 后继续修改；root 需 fresh compile，并重跑相关 exact 用例。当前不能声称 API、WS、路由 auth 单测或 report-corrections 已全绿。

## host35 短测与 fixture 诊断

host35 manifest 为 `.tmp/checklist-wave4/host35-binaries.json`，独立日志位于
`.tmp/checklist-wave4/server-identity-migration-host35/`。现有 fresh binary 的 API、WS 与路由 auth
用例仍有失败；其中 API 删除会话测试曾把 JSON generation 序列化为含引号的 URI 参数，现改为读取
`as_str()`，需 fresh binary 复验。WS queued command 的 request_id 断言失败，诊断已增强，需 fresh
binary 复验。

路由 auth 单测显示 `initial_allocation` 对有效、未加入市场的 guest 返回 200；路由通过
`authorized_market` 授权，故该共享市场只读查询允许市场成员读取，测试现改为验证 200 与 generation，
同时保留无凭据 401、过期 generation 冲突和伪造账户参数拒绝。私有 HTTP 路由测试的 intent 返回 400；
fixture 默认开局日为 2030-01-01（闭市日），现显式设为 2030-01-02 后须用 fresh binary 重验。
旧 binary 不能作为这些修改的通过证据；本轮未运行 Cargo。

## host36 精确失败与最终 fixture 修订（待新 binary）

host36 manifest 为 `.tmp/checklist-wave4/host36-binaries.json`，四个指定用例日志在
`.tmp/checklist-wave4/host36-fourcase/`。删除会话测试 1/1 通过；两条 API intent 和 WS gateway
用例均真实显示默认 2030-01-01 是 SSE 模拟新年休市日。generation API 用例另明确显示 restore 后
没有该身份的成员关系，不能把闭市与 membership 缺失混为一因。

API/WS `sample_setup_json` 现显式采用 2030-01-02。generation API fixture 保留 2030-01-05 周六的
真实 `ProtocolSession`，对候选分别执行周六、周日日终更新，断言实际民用日期推进到 2030-01-07 后
再 restore；因此不依赖仅在 `host-parity` feature 下存在的 route。用例保留陈旧／缺失／非规范
generation guard，断言 restore 后成员缺失时 intent 被拒绝，再经真实 `/api/markets/join` 携带
`confirmed_rejoin: true` 加入。它校验真实 AccountID、`AdmissionFunding.external_cash` 与当前
starting cash 一致，并以重复 join 验证既有 AccountID 和入场资金不变；最后用当前实际 context
generation 检查有效交易意图。上述修改尚待 root fresh compile 和 exact 用例复验，不声称通过。

## host37 partial 精确结果

host37 整体 no-run 因该测试首次 `market_context_for(subject)` 消费 `subject`、后续还需使用而
报告 E0382。现仅在首次查询处改为 `subject.clone()`，没有改业务断言。非作者静态复核确认该修正
只修复所有权移动。host37 partial manifest `.tmp/checklist-wave4/host37-partial-binaries.json`
的 fresh WS 与 server lib binary 已按 `--list` 核实精确用例后并发运行：WS gateway 1/1、路由
auth 的 initial allocation、private HTTP、intraday/history 各 1/1 通过，单项耗时 0.20–0.22 秒，
日志在 `.tmp/checklist-wave4/host37-partial-exact/`。API contract binary 未出现在 partial manifest，
未使用旧 API binary；修正后 generation intent 用例仍待 root 新编译与复验。

## host38 API/WS 精确验证与空响应修正

host38 manifest `.tmp/checklist-wave4/host38-binaries.json` 包含 API contract fresh binary。逐一
`--list` 后，按最多 8 进程并发、每项外部 9 秒、每批外部 10 秒执行，日志位于
`.tmp/checklist-wave4/host38-exact/results/`。API 删除会话与实际存在的 7 项 speed case 全通过；
WS baseline、gateway、缺失/查询凭据、subprotocol credential 4 项通过；report correction routes、
failure retry 2 项通过；三项 lib auth、pause security 与 deployment identity route 均通过。WS
`publisher_modes_report_actual_speed` 被精确列出但标记 ignored，不计为绿色 case。

两项 API 成功 intent (`intent_known_session_returns_200`、`intent_requires_current_canonical_generation`)
在 host38 的真实失败是测试用 `response_json` 试图解析合法 HTTP 200 空响应；生产 `api_intent` 的
成功分支明确返回 `StatusCode::OK.into_response()`，intent 是 void acknowledgment。现在只修改测试：
成功断言 200 且 body 为空；非 200 时解析并显示实际 JSON 错误。旧 generation guard、缺 membership
拒绝、真实 join 与 `AdmissionFunding` / AccountID 幂等断言不变。该两项新断言尚待最新 API binary
复验，不将 host38 误解析失败写作绿测。

## host39 最新 API exact 结果

host39 fresh manifest `.tmp/checklist-wave4/host39-binaries.json` 明确包含
`api_contract-656ecb27491f49bc`。对 binary 执行 `--list` 核实后，将两条 intent、新建、删除与四项
speed 用例作为 8 项分片；剩余三项 speed 用例作为 3 项分片。每项进程外 deadline 9 秒、每分片
进程外 deadline 10 秒，并发 8、`RAYON_NUM_THREADS=4`。日志位于
`.tmp/checklist-wave4/host39-final-api/`。两条 intent（包括完整的 stale generation、恢复后真实
membership rejoin、资金幂等与有效交易意图断言）、new、delete 和当前 API target 实际列出的全部
7 项 speed 用例均为 1 passed、0 failed；最长 0.68 秒。该 host39 结果是空响应测试修订后的 fresh
绿色证据，不采用 host38 两条测试 helper 误解析空 ack 的失败日志作为当前结论。

host39 独立复核未发现行为或断言问题；指出 auth 单测名 `initial_allocation_route_requires_owner...`
与当前“有效凭据即可查询共享市场信息”的断言不符。现改名为
`initial_allocation_route_requires_authentication_and_current_generation`，只有名称变化；host38/39
此前的绿色日志对应旧名，最终命名仍需 fresh `--list` 并以新名称 exact 复验。

host40 fresh manifest `.tmp/checklist-wave4/host40-binaries.json` 的 API binary 为
`api_contract-11218a2fbd3e2d7a`、server lib binary 为 `server-5bddaa31e4b2ee35`。重新 `--list`
确认两个 API intent 用例及重命名后的 allocation route auth 用例，再以 3 并发、每例外部 9 秒、
整批外部 10 秒运行；三项均 1 passed、0 failed，日志位于
`.tmp/checklist-wave4/host40-identity-final/`。
