# 沪深 2026 年度休市安排取证与 TDD 记录

- 主题分支：`codex/calendar-2026-official-20261102`
- 基线：本地 `main` / `0f40ffbc2dfd11db3e8ac72faa23ac3643b699b9`
- 首次取证日期：`2026-11-02`（Asia/Shanghai）
- 公告发布日期：两所均为 `2025-12-22`
- 后续复核时间：`2026-11-02 16:21 UTC`（本地已进入 2026-11-03）；未把该次时钟读数改写为新的首次取证日

## 官方原文

| 交易所 | 文号 | 稳定性 URL | 页面发布日期 |
|---|---|---|---|
| SSE | 上证公告〔2025〕45号 | <https://www.sse.com.cn/disclosure/announcement/general/c/c_20251222_10802507.shtml> | 2025-12-22 |
| SZSE | 深证会〔2025〕481号 | <https://investor.szse.cn/disclosure/notice/general/t20251222_618087.html> | 2025-12-22 |

两页均由 `curl -L --fail --silent --show-error --max-time 20 -A 'Mozilla/5.0' URL` 直接取得，正文逐段阅读。SSE 页面正文位于 `div.allZoom`，SZSE 页面正文位于 `div#desContent`。

规范化摘要方法：提取正文 `<p>`，去除 HTML 标签与实体，`&nbsp;` 转普通空格，连续空白折叠为一个空格，丢弃空段，按原文顺序以 LF 连接并在末行保留 LF，UTF-8 编码后计算 SHA-256。规范化正文已保存在本目录，可按同法重算。

| 交易所 | 当次页面原始字节 SHA-256 | 规范化正文 SHA-256 | 规范化正文 |
|---|---|---|---|
| SSE | `ff188c710b8904758c185eb955e9eaa2c9bd7c2061af984d4f1d743f899c12a4` | `3ecade6151ea19fe223b747eea08212be430f69ae7a52056f1961b2087e939bc` | `sse-2026-holiday-notice.txt` |
| SZSE | `1656d8708d5faed33c7588538639d96128f39fdface02cfa6eb66d796c40f14e` | `87ecf6071c1aa57eb6dc70327e7d763481976300079d622739b27766b789535f` | `szse-2026-holiday-notice.txt` |

页面原始字节摘要绑定的是当次响应，可能受页面模板和动态资源影响；运行时 `source_digest` 与 fixture 使用稳定的规范化正文摘要。SZSE 正文明确引用中国证监会《关于2026年部分节假日放假和休市安排的通知》（证监办发〔2025〕130号）。

## 两所公告日期表逐项核对

| 节日 | 公告休市区间 | 区间后开市日 | 公告另列的周末休市日 |
|---|---|---|---|
| 元旦 | 2026-01-01（周四）..=01-03（周六） | 01-05（周一） | 01-04（周日） |
| 春节 | 2026-02-15（周日）..=02-23（周一） | 02-24（周二） | 02-14（周六）、02-28（周六） |
| 清明 | 2026-04-04（周六）..=04-06（周一） | 04-07（周二） | — |
| 劳动节 | 2026-05-01（周五）..=05-05（周二） | 05-06（周三） | 05-09（周六） |
| 端午 | 2026-06-19（周五）..=06-21（周日） | 06-22（周一） | — |
| 中秋 | 2026-09-25（周五）..=09-27（周日） | 09-28（周一） | — |
| 国庆 | 2026-10-01（周四）..=10-07（周三） | 10-08（周四） | 09-20（周日）、10-10（周六） |

两所正文的七个节日、全部区间端点、恢复开市日及“另外”周末休市日完全一致。周六、周日仍由基础周末规则判休市；`closed_ranges` 同时登记公告明示的周末休市日，以便完整绑定通知，不把政府调休上班周末误当交易日。

## 与升级前模拟回退的差异

2026 从 `notice-text-unverified` 升级为 `Official` 后，新增的“工作日由官方公告休市”边界为：

- `2026-01-02`
- `2026-02-20`、`2026-02-23`
- `2026-04-06`
- `2026-05-04`、`2026-05-05`
- `2026-10-05`、`2026-10-06`、`2026-10-07`

`2026-02-16..=02-19`、`2026-06-19`、`2026-09-25` 等日期在旧模拟规则下也休市，升级后原因改为 `OfficialHoliday`。2025 及以前、2027 及以后不使用 2026 官方条目；测试逐日比较 1998–2025 与 2027–2099 的 SSE/SZSE 状态，确认未受本次升级影响。

## TDD 红绿记录

构建与执行分开：

- 构建：`timeout 300 env CARGO_BUILD_JOBS=32 cargo test -p engine --test calendar --test policy_manifest --test audit_restore_guards --no-run`
- 普通测试：`timeout 10 <prebuilt-test-binary> --test-threads=1 [...]`

红灯在实现前实际取得：

- `official_2026_boundaries_match_both_exchange_notices`：`NoticeTextUnverified != Official`，0.06 秒。
- `frozen_calendar_survives_new_defaults`：默认政策尚无 2026 Official，0.01 秒。
- `policy_constants_match_frozen_fixture`：引擎 official_coverage 为 0、fixture 期望 2，0.01 秒。
- `rejects_wrong_digest_on_restore`：默认政策缺 2026 Official，0.01 秒。
- `policy_manifest` 的新取证批次结构校验在红灯阶段已 6/6 通过。
- 独审发现真实 SaveSlot digest 缺口后，先增加三条失败测试：
  `frozen_policy_disclosure_and_retention_invariants_hold_on_real_saves`
  因存档无 `policy_digest` 失败；`tampered_official_source_digest_is_rejected_by_real_save_restore`
  与 `tampered_official_closed_range_is_rejected_by_real_save_restore` 都因篡改档仍能恢复而失败。
  三者分别在 0.45、1.15、1.17 秒内因正确原因变红。

实现后绿灯（全部为已构建二进制直接执行，外层 `timeout 10`，单进程 `--test-threads=1`）：

- 构建：`timeout 300 env CARGO_BUILD_JOBS=32 cargo test -p engine --lib --test session --test civil_clock --test calendar --test policy_manifest --test audit_restore_guards --no-run`；随后独立构建 `mixed_calendar_*` 与 `save_contract` 三个/一个测试二进制。
- `calendar`：16/16，0.18 秒。
- `policy_manifest`：8/8，0.02 秒。
- `audit_restore_guards`：10/10，0.49 秒。
- `civil_clock`：19/19，4.72 秒。
- `session` 中冻结日历/逐股开市两个用例：2/2，0.80 秒。
- runtime 日历分组：4/4；reopen：1/1；混合开休信息交集：8/8；information clock context：3/3；cancel-all：5/5；chain-caller：8/8；projection：3/3；public restore：8/8；final boundary：4/4；企业资本冻结日历：1/1。
- `calendar_entry_cache_tests` 6 个与 `calendar_nonzero_plan_parity_tests` 8 个逐 case 执行，14/14 通过；每个 0.72–5.29 秒。
- `mixed_calendar_c2c`：9/9，5.30 秒；`mixed_calendar_c2c_feedback`：2/2，0.91 秒；`mixed_calendar_feedback`：17/17，7.51 秒。
- `save_contract` 43 个用例按模块/单 case 分片全部通过；完整 42 用例批次曾在 10 秒外层 deadline 收到 exit 124，未计通过。新增关键绿灯：存档 `policy_digest` 为 16 位十六进制；篡改 `source_digest` 或 `closed_ranges` 均由真实 SaveSlot 恢复拒绝；携带官方覆盖却缺 digest 的档拒绝；缺失 digest 但 spec 精确等于升级前默认政策的旧档可恢复并重新保存补 digest。
- Web schema：`civil-clock.test.ts` 5/5；`save-schema-contract` 32/32；`current-wire-contract` 11/11；`complete-save-schema` 4/4；`complete-save-schema-extra` 1/1；均以 Node 24.18.0、`timeout 10`、`--test-timeout=10000` 直接运行。
- `tsc -b --pretty false` 通过；定向 oxlint 通过；`cargo fmt --all --check` 与 `git diff --check` 通过。

## 独立审查发现与修复

首轮独审确认官方日期、来源摘要、2026 外模拟年份、旧标签和排序调整均正确，但发现实际 `SaveSlot` 只保存 `CalendarPolicySpec`，恢复时重算 content digest，没有比较持久化 digest；因此篡改 `source_digest` 或保持出处而改 `closed_ranges` 会静默成为新合法政策。

修复：`CivilClockSave` 新增新档必写的 `policy_digest`；恢复时先由 spec 全量校验并重算 digest，再与存档值严格比较。升级前缺 digest 的旧档没有任意放宽：只接受 spec 精确等于升级前默认政策（官方覆盖为空且算法版本、边界、回退数据全部一致）的冻结模拟政策，并经过全部内层 digest 校验；任何官方覆盖、字段改写或缺摘要均显式拒绝。恢复后再保存会补齐新 digest。真实完整存档测试覆盖 source digest、closed_ranges、缺 digest 与旧档精确白名单。

诚实边界：曾把 141 个名称含 `calendar` 的 lib 用例合成一批，因超过 10 秒进程树 deadline 收到 exit 124；该批未计为通过。随后只运行本 diff 影响的模块，并把两个超过 10 秒的模块拆成 14 个单 case，全部在 10 秒内完成。没有放宽 deadline，也没有把超时分片的部分输出当作通过。

并发排序说明：默认政策现含 `[SSE 2026, SZSE 2026]`；测试向尾部追加 SSE 2030/2031 后再按 `(exchange, year)` 排序，保持生产 `from_parts` 的严格排序拒绝不变。依赖 `[0]` 的 synthetic 来源篡改用例改为按 `source_citation_id` 定位；原有断言保留，另增加“新默认不得覆盖旧档”“2026 旧标签可恢复”等断言。
