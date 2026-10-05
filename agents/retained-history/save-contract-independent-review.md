# Web 与原生存档契约独立复核

## 范围与结论

本次仅复核 `retained-history.ts`、其测试、`root.ts` 中永久历史接线、`runtime-state.ts` 中活动分钟字段、`day-end-candidate.ts` 的日终 guard，以及 `packages/native-store/tests/archives.rs` 中新增的原生拒绝用例。没有实施代码，也没有运行测试或编译。后续复核了测试文件追加的零成交与日期连续性用例。

整体设计预计可以满足本批目标：Web parser 对历史自然日连续覆盖、证券集合、稀疏成交分钟、配置阶段、Tick 精度及日 K 合计建立了严格校验；原生用例针对加载与复制入口拒绝非空活动分钟，并核对原始 payload 未被改写。当前没有发现会使已覆盖正常路径接受错误分钟总量或伪造分钟事实的确定性漏洞。已检查原生新增 case 的真实运行日志并确认通过；Web 正规存档 fixture 与跨层完整验收状态仍需由 root 记录，不据此扩大本次签核范围。

## 语义核对

- 分钟标签使用上海市场时段：开盘集合竞价成交标记为 09:25，连续竞价为 09:30–11:30 与 13:00–15:00，收盘集合竞价成交标记为 15:00；收盘竞价启用时，14:57 起不接受连续竞价成交。该口径与 `agents/retained-history/core-contract.md` 一致，没有在 Web 层复制农历或交易所日历。
- OHLC 均为正分整数且落在本证券 tick 上，量、笔数及成交额用规范十进制字符串解析；稀疏 bars 不填充无成交分钟。允许 `Trading` 空 bars 表示零成交，要求 `Closed` 无 bars，并以日 K 的存在性、量额笔数及 OHLC 汇总互证。
- 审查未新增交易制度规则；以上语义依据本项目 ADR-0034 与 core contract。交易所日历仍由 Rust `ProtocolSession.restore` 权威校验，Web 的状态与日 K 互证不能替代它。

## 必须修复

目前没有能据此要求作者必须修复的确定性领域漏洞。

## 建议补足的边界

- `parseActiveMinuteHistory` 单独调用时只拒绝空证券代码；不校验代码格式、是否属于会话证券或对应证券的 tick/config。`parseStrictSaveEnvelope` 后续会校验证券存在并重新按股票与配置解析，因此完整存档入口有保护；建议至少增加 root 集成负例，固定未知证券及错 tick 活动历史会被拒绝，避免未来调用方绕开 root parser。
- 后续新增测试已覆盖 `Trading` 空 bars 加零成交统计可通过，以及跨闰日、跨月的连续自然日解析；这些日期仅作为 CivilDate schema fixture，不声称是实际交易日，权威交易日历仍由 Core 校验。仍未显式覆盖错误证券 tick、关闭的竞价阶段等 root 接线边界。core contract 已记有 Engine 竞价负例，Web 层仍应固定自己的 parser 行为。

上述属于测试覆盖建议，不阻止当前实现目标；若随后发现 root fixture 表明这些边界不满足，应升级为必须修复并复核。

## 必要性与范围

严格存档 schema、必填活动分钟状态、日终拒绝 guard 都是 ADR-0034“仅日终持久化”和完整历史留存的必要接线。新增 native test 验证加载/复制拒绝且不篡改原事实，范围直接。`root.ts` 与 `runtime-state.ts` 同时含有本任务以外的改动，本次不把这些共享差异视为本审查作者范围，也不以其测试状态作判断。

## 最终复核与验证状态

- 作者转述 Web 新增测试共 6 case 短测已绿（追加后 265ms）；本审查只静态复核新增用例，未独立执行。
- 已亲读 `.tmp/checklist-wave4/retained-history-native-guard-host44.log`：`archive_load_and_copy_reject_active_minutes_without_rewriting_facts` 为 1 passed、0 failed，耗时 1.56s。代码确实先保存有效日终档，再植入非空 `active_minute_history` 到现存 payload；`load` 与 `copy` 均报错、复制目标不存在且源 payload 原样不变。该日志是 root 授权的实际执行证据；我未重跑测试。
- 本轮复核 `day-end-archive.test.ts` fixture 修复：先前 helper 将 clock 设为 2030-01-02 / settled 2030-01-01，却没有对应 retained day，导致 6 项日终断言都在 schema 前置校验失败。`.tmp/checklist-wave4/retained-day-end-fixture-red.log` 印证 6/6 失败均为历史覆盖缺失。修复只在测试 fixture 加入 2030-01-01、证券 `600101` 的 `Closed` 空 bars 日；没有生产改动。该日是测试状态 fixture，Web 不据此认定交易所日历。
- 该测试文件新增明确历史覆盖拒绝断言：空历史拒绝、缺失证券拒绝；既有日终活动订单、资源 envelope、未处理请求和未完成 parent order 断言均保留。`.tmp/checklist-wave4/retained-day-end-fixture-green.log` 显示新增及相邻 guard 共 8/8 通过，耗时 292.743ms。本人亲读日志与差异，未重跑。
- Web 普通 retained-history 短测此前结果仍是作者转述；真实 JSON fixture 及其余跨层验收不由本次测试覆盖，不能记为通过。
- 未运行官方交易所规则查证：本批是既定分钟时间标签的 schema 校验，未改变交易制度；制度权威复核仍由 Engine/restore 与项目规则文档负责。

## 签核

在本次指定复核范围内，新增 Web schema、日终 archive fixture/guard 与 Native guard 的目标及语义预计成立；本次未发现必须修复项。Native 加载/复制拒绝用例及 Web 日终 fixture 修复后的相邻短测均有实际通过证据。独立审查门禁对该范围予以通过；普通 retained-history 短测及未完成的真实存档/跨层验收仍须按各自证据单独报告。
