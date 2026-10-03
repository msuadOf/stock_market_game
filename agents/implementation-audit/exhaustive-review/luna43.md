# 三份实现盘点文档全文与调用链复核

- 审查提交：`08e4fc75b52a71a3262a8a938c57b44f8b5b4960`；checkout HEAD 是合并该提交后的 `a7c7ce3`。
- 全文读取目标：`docs/implementation-gaps.md` 259 行、`docs/naming-conventions.md` 48 行、`docs/naming-refactor-validation.md` 62 行；均逐行读至 EOF。
- 目标提交相对代码审计基线 `2247f4f` 只新增两份 `agents/main-release-validation/` 验收记录，没有产品源码差异；因此沿用实现复核记录的生产路径，再核对目标提交里的最终验收记录。未运行测试、构建、联网规则核验；不把文档中的历史测试记录说成本轮运行。
- 规范依据：`AGENTS.md` 与 `docs/principles.md`。交易域以 ADR-0025/0026 现行存档、个体机构经历决定优先；无新增交易规则结论。

## 逐章复核：implementation-gaps.md

| 行 | 章节/原文锚点 | 当前实现或反证 | 结论 |
|---|---|---|---|
| 1–10 | 顶部称“最新审计…优先”；2026-10-01 更新 | 目标代码与 `agents/implementation-audit/implementation-audit-2026-10-02.md` 覆盖至 `2247f4f` 的复核一致；目标提交没有其后的产品改动。ADR-0025 明确自然日日终候选、宿主 generation 与内存日内活动订单边界。 | 优先级陈述仍准确；不能把 A 表历史状态当当前待办。 |
| 11–58 | 实施更新 A01–A11、A05 恢复证据及 Continuous 失败根因 | 当前调用链支持“主要能力已交付、局部仍有 G 缺口”：A01 `packages/engine/src/company/query.rs`→公开报告呈现；A02 `apps/web/src/host/remote-host.ts`→真实公司查询路由；A03 `packages/engine/src/session/decision_chain/roots.rs` / `plans/urgency`→机构信号及恢复；A04 `UrgencyPolicy` 进入 session/save/recovery/quote；A05 `session/persistence.rs` 的日终 DTO 重建派生态；A06 `session/company_operations.rs` 日界候选/回滚；A07 App 桌面面板布局；A08 `host/protocol-failure.ts` 与 Rust HostFailure 生产/解析；A09 Rust indicators 到 `use-indicator-results`/日 K 绘制；A10 protocol account/order delta 到 Redux；A11 `NpcDecisionInspector` 读取当前 host 并用 generation 门控。实现审计 G01–G39 说明这些总体能力不能抹去已确认的局部断点。Continuous 的原失败读取 `snapshot().accounts` 只投影玩家；当前测试改用 `snapshot_inner(true, true)`，300/200 股等断言保留，旧失败不得继续描述为当前源码失败。 | 现行语义与总账优先级正确。页面提到的源码提交/验证是历史证据；不是目标提交重复实施。 |
| 59–79 | §1 方法与结论边界；称全文 134 文档、Git 132 Markdown、3 草稿 | 明确承认“不是逐字审阅全部源码”、没有跑完整验收，结论边界合理。该段是 2026-10-01 那次工作的历史方法记录；后续 `agents/implementation-audit/coverage-index.md` 已更新为 142 个现存来源路径、3 草稿、2 删除历史路径，不能将本段数字误读成目标提交当日的现行覆盖总数。 | 无实现漏项；后续审计规模已扩充，应让读者按时间和 coverage-index 理解，不可合并成同一批覆盖声称。 |
| 80–98 | §2 A01–A11 首次盘点表，明言历史发现 | 逐条回查：A01 初次“公共 DTO 只合计”的状态已由完整报告产物/附注/比较关系更新；A02 初次 RemoteHost 路由描述已由现行 route/query coordinator 更新；A03 初次 false/None 风险输入由个人机构 root 接线更新，但散户/个体其它 G 项仍在；A04 默认策略被必填冻结字段取代；A05 展示 snapshot 存档被 ADR-0025 日终事实 DTO 取代；A06 SaveSlot 备份换成内存检查点；A07 静态布局换为六面板拖拽缩放（真实浏览器矩阵仍未做）；A08 HostFailure 已有脱敏详情/复制及 Rust source/context；A09 MACD/KDJ Rust 值接入宿主，桌面日 K 输入修正仍需别处确认；A10 玩家账户/活动订单 delta 已穿透 reducer/UI；A11 DEV inspector 从独立模拟会话变成当前 host 查询。 | 初次描述明确标成历史后，不构成重复缺口；二次更新应与后续 G 表一并读。 |
| 99–126 | §3 B01–B20 范围未定/未来产品 | 全部逐项以当前产品入口/决策复核，见下节 B 矩阵。B01/B07/B20 特意标记已实现/核销；其余不是现行实施授权。 | 分类整体与最新决定一致；只需避免把 disabled/未来项误报为 bug。 |
| 127–150 | §4 明确不模拟/简化 | A 股业务简化按 `docs/trading-rules.md` 的现有登记陈述：公司行动、NPC 注资、特殊证券/市价申报、保险/银行扩展等。没有把简化错说成已实现规则，也未发现某个新缺口仅靠本节文字即可成立。 | 保留。交易所规则未重新联网核验。 |
| 151–171 | §5 规则数据、法源与验收证据债 | 区分数据依据/真实宿主验收缺口与游戏功能实现；这与实现 audit G/Q 分类一致。必须继续保留“没有本轮运行”的限制。 | 保留；不将旧测试名当当前覆盖结论。 |
| 172–191 | §6 过时报法排除 | 服务器容量、休市午休观察时钟、公司与 NPC 现有能力、指标/UI、并行、jemalloc、会话鉴权、调度时序、PlanBook 私有候选、旧 API/字段演进均有当前来源或 ADR 反证。尤其 ADR-0017/0018 允许实际局部受理顺序变化，不能要求自由并发完整输出同序。 | 反证成立；G39 仍指出验证工具的跨 worker 整产物相等契约过严，这是工具缺口而非交易生产实现缺陷。 |
| 192–259 | §7 全文阅读覆盖表 R1/R2/R3 | 已逐行检查全部矩阵条目；它记的是各批原始正文行数和历史覆盖，不是本次实现验收。后来的 coverage-index 有更新版本、更多来源和代码复核记录。 | 覆盖清单作为当时记录有效；不能当作 08e4fc7 的最新全仓 Markdown 清单。 |

## A01–A11 当前调用矩阵

| 项目 | 现行旧结论复核 | 当前生产入口/反证 | 最小范围判断 |
|---|---|---|---|
| A01 | 首次“合计 DTO/少量行拼表”已过时 | `packages/engine/src/company/query.rs` 报告查询进入 Web 公司报告协调器和呈现组件；公司披露产物仍限制在已公开报告。审计记录确认五类公开产物、附注、范围和更正关系已交付。 | 仅剩公司 E2E 未跑的证据边界，不可重开已实现报表。 |
| A02 | 初次 RemoteHost 路由/分页/按 ID 缺失已过时 | Web `company-query-coordinator`→RemoteHost→Server company route，带 token、分页、ID 查询；读档会失效查询缓存。G01–05 是远程 WS 授权/取帧/重连等独立问题，不能错并入财报查询。 | 财报查询已接线；远程宿主其它 G 不影响本项结论。 |
| A03 | 主要机构风险与 pause/resume 缺口已补 | `InstitutionDecisionRoot` → `BeliefBook`/个人经历→`UrgencyPolicy`→计划报价/执行；ADR-0026 对账户暂停、确认恢复、冻结参数和幂等作边界。G37/G38 及散户 G07–09 属另一层尚存细节。 | 不重做账户风险模型，不外推为所有代理策略已完成。 |
| A04 | 每次默认策略旧结论已核销 | 必填 `UrgencyPolicy` 已进入 session/save/restore/clone/hash/P2/quote；生成 TS 类型仍是七字段策略结构。 | 证据支持交付；不添加现金留底/仓位上限。 |
| A05 | snapshot 全量存档旧结论已核销 | ADR-0025 日终候选键→三宿主 generation 门控→候选解析；setup/玩家账户/公开日期事实持久化，订单簿/活动挂单等派生或日内态按恢复路径重建/排除。 | 单项恢复用例与完整验收记录不改变“仅日终持久化”语义。 |
| A06 | `SaveSlot` 备份/restore 旧结论已核销 | 自然日日终使用内存检查点、完整对象恢复；observer 已运行的外部副作用不承诺回滚。 | 与公开快速槽/文件持久化分开，最小且符合 ADR-0025。 |
| A07 | 纯静态桌面布局旧结论已核销 | 六个面板接入 drag/resize 与容器测宽，`App` 按 desktop/mobile layout 保持不同布局规则；窄屏持仓区域已由实施记录修正。 | 实际拖拽浏览器矩阵仍未跑，不能升格为 UX 验收通过。 |
| A08 | 只有 code/where/message 旧结论已更新 | `HostFailure` 在 Web parser 支持 context/cause/recoverable/recoveryActions；三宿主 Rust 错误生产者携带真实 where/context/source，未知错误不直接暴露 Display。 | source 不存在时 null 是诚实状态；不应强造错误链。 |
| A09 | “MACD/KDJ 仅 Web”旧结论已核销 | Rust 指标服务经 WASM/Server/Tauri 入口到图表。桌面日 K 使用日收盘/OHLC/日轴，分时保持分时数据；G17 是引擎逐证券 indicator batch API 未接三宿主，和玩家图表指标已接入不同。 | 保留 G17，不误称 MACD/KDJ UI 缺失。 |
| A10 | runtime snapshot 整体替换旧结论已更新 | Host account/order reset/upsert/remove delta→protocol reducer→Redux→订单 UI；baseline/CivilUpdate 全量边界仍保留。 | wire 新测试红灯时序证据欠缺不代表生产调用链未接。 |
| A11 | DEV 独立 WASM 会话旧结论已核销 | 当前协商 `EngineHost.npcDecisionTrace`→`NpcDecisionInspector`；切档按 host/timeline generation 使旧 success/catch/finally 失效；release capability 不带私有 trace。 | 三宿主浏览器矩阵未执行；G37 诊断真实订单关联是另一生产缺口。 |

## B01–B20 当前范围/调用矩阵

| ID | 当前引用/调用反证 | 结论与旧失败复核 |
|---|---|---|
| B01 | `apps/web/src/utils/symbolic-limit-order.ts`、订单面板提交 `PlaceLimit`，引擎受理解析最高/最低符号价 | 已实现；仍是限价单，不是市价或追价。曾有 helper 加载失败的红测，不可说断言红测通过。 |
| B02 | `MobileStockDetail.tsx` 五日周期按钮 `disabled` 并显示“等待引擎提供跨日分钟数据” | 未实现、明确待数据；完整日 K 不代表跨日分钟链。 |
| B03 | 同组件更多周期/均线设置按钮 `disabled`；现有日/周/月 K 展示不是已开放全部图表选项 | 未来 UI 产品范围，未定义选项，非意外断链。 |
| B04 | 个股信息占位与 `MarketGrid` 禁用入口；盘口、个股资金统计已有生产组件 | 内容/首页快捷页/更多行情分类未实现且待产品定义；不能用“有 tab/占位”核销内容。 |
| B05 | `docs/decisions/0014-closing-call-auction.md`；日 K 接收收盘真实结果，无独立收盘竞价坐标轴 | 仅视图未扩展，收盘撮合不是缺功能；新增该 UI 需决策。 |
| B06 | `SaveRepository` 单快速槽、`store.ts` 最新 100 笔成交投影 | 多槽、成就、完整交易流水/复盘、云同步均属 Stage 2 开放问题。 |
| B07 | RemoteHost HTTP/WS 私有请求带 bearer；`apps/server/src/routes.rs` 对会话身份校验，失败在访问数据前返回 | 原“无认证”旧结论过时，当前私有会话授权闭环。公网账号/多人身份仍未决定。 |
| B08 | `apps/server` Actor/DashMap 保存进程内会话；save/load API 传输 ADR-0025 日终档 | DB、迁移、重启恢复未实现且 roadmap 待决；不混淆传输档案与服务端持久化。 |
| B09 | Server `main.rs` 直接 HTTP serve、`lib.rs` CORS Any；有 tracing/healthz | 公网 TLS/Origin 白名单/运营限流方案未完成；现有 health/log 不等于零运维。不得恢复任意挂单配额伪作保护。 |
| B10 | Tauri bundle 配置存在、未见 updater/signing 链 | 签名、自动更新与分发链待平台/证书产品决策；桌面壳已经存在。 |
| B11 | `docs/open-questions.md` 明确首发中文、不引入 i18n | 二语言未实现是未来范围，不是当前矛盾。 |
| B12 | `packages/engine-gpu` 能力仅探测/委托 CPU；ADR-0008 分开实时计算与离线回测 | GPU kernel 与 GPU Monte Carlo 未实现；GPU 实时 decide 明确不做。 |
| B13 | ADR-0018 对冷历史、区间查询、逐笔归档属后续方案；日 K 追加保存现存 | 长期冷热分层/分钟逐笔历史未实现，粒度与保留期限待定。 |
| B14 | `session/pipeline/shadow.rs`、root capture 和 `PlanBook` 仍克隆历史计划；账户页与日 K 分享已存在 | 全面页级 COW/状态根等 ADR-0018 提案未落地；不能笼统称所有历史每 tick 深复制。 |
| B15 | `protocol/reduce.ts` accepted Map 克隆、日内帧追加复制；终端版本清理/去重期限未定 | 完整反向索引和增长策略未实现；不要为省内存删权威防重事实。 |
| B16 | ADR-0010 明示无 WAL/crash recovery，ADR-0018 可靠投递条件方案 | 当前无 WAL/watermark/强发布；这是未授权的更高持久化保证，不是日终档漏实现。 |
| B17 | 当前 generation/seq 门控存在，未形成每请求玩家观察 token、历史私有版本查询/过期策略 | 这些未来查询语义未定，不能因已有 generation 就称历史观察全实现。 |
| B18 | `docs/decisions/0018...` 范围到 2099-12-31 | 2099 后日历规则延续明确未决定，不以增大日期整数代替官方规则包。 |
| B19 | 量价清单研究候选；当前混合分析、个人记忆、失败衰减、账户回撤、持续计划已有 | 复杂社会学习/L2/多尺度模型等未批准扩展；不新增真实资金补注、公共仓位上限或强制止损。 |
| B20 | ADR-0025 日终档只载日级事实，活动委托留内存 | 已核销；“恢复现金与日内买单冲突”在该持久化边界不存在。非法事实仍显式报错，不补钱/撤单。 |

## 命名约定与验证记录

### `naming-conventions.md`（行 1–48）

- §开头到 24 行的职责命名示例与版本名例外：源码实际保留 `P0`/`P3Created` serde 拼写，市场、序列化/API 真实版本号不随实施任务编号重命名；当前 Rust 代码保留 wire 格式且 Web parser 消费同一枚举。旧事件来源不能从新变体名倒推，符合存档/API 稳定性。
- 26–41 行保留旧 K7/evidence/hash/identity 格式，与源码职责变量区分。矩阵运行身份有意在生产者及消费者同步从 task9 改为 `escrow-verification-matrix`；依据文档所记改名提交，不等于篡改 sealed evidence。
- 43–48 行称模块/变量改名不改交易数量、货币/股数单位、T+1、费用、价时、集合竞价、释放可见性或原子提交。核心/流水线契约复核及目标提交无生产 diff 支持没有发现反例；此为静态审阅，不替代实时交易规则来源核验。

### `naming-refactor-validation.md`（行 1–62）

- §1–23 保存六组提交、Serde/历史压缩档和提交原因，作为 2026-10-01 的改动账本，不用来推断当前 HEAD 的未提交变更。
- §24–40 的局部验证结果是原始历史记录。目标提交更新测试所对应冻结源码的总验收：`agents/main-release-validation/summary.md` 记录默认 Rust/Web 回归、all-feature、scripts 32/32、Chromium E2E 12/12、格式/类型/生产构建/Clippy，结果针对 `2247f4f` 冻结源码；当前目标提交只有验收文档差异，故这不是本 subagent 重跑。
- §41–62 六条“未通过扩展检查”均要与新结果逐项相联：
  - Server 13 个 `handles.save()` 缺参数：summary 后续完整默认和 all-feature 工作区回归最终通过，源码冻结为 `2247f4f`，故旧编译失败已被后续源码/fixture 修正解决，不能继续称为当前失败。
  - Web `wasm-worker-failure.test.ts` 七项消息名不符：后续 608 个 Web case 全通过，旧失败不再是当前源码状态。
  - escrow harness 两个 zero-based/contiguous 校验失败：scripts 32/32 通过，且 `escrow-verification-contracts.test.mjs` 与 `run-escrow-verification-matrix.test.mjs` 通过；主 matrix 实际长验收结果仍不能由单元测试替代。summary 记完整 K7 比较仍有设计范围外事项，须看 G39。
  - `multi_round_trades_record_first_real_open_and_final_depth_once`：完整 Rust 回归最终 2199 case 通过；旧失败不能保留为当前红测。
  - 两个 `company_scenarios` 超 10 秒：最终完整验收的跨年/公司等相关普通 suite 成功记录覆盖该轮执行；summary 记 2199 Rust case/74 binary 通过。原来被 deadline 终止的执行仍是历史事实，不能改写为当时通过。
  - 类型导出合并命令超时：最终格式/生成 TypeScript 契约检查通过；初次大命令 timeout 的记录依然真实，但之后独立验证已通过，不可把初次方式超时等同契约失败。
- 独立证据边界：`agents/main-release-validation/summary.md` 声明最终 Rust runner 没有每个普通 case 独立 10s watchdog；“批次通过”不能证明每 case 满足硬时限。Web lint 有三个 warning，不应说零 warning。最终回归/构建/浏览器不是本审查执行。

## 总账漏项、候选与反证

1. A/B 表本身是覆盖率边界，不是当前全缺口总账。复核到的 39 个 G 仍由 `implementation-audit-2026-10-02.md` 与领域复核分别承担；本次没有以 20 个 B 未来项增加当前缺口编号。具体 G 的稳定生产断点包括远程连接消费、个体零售学习、集团报告、日历覆盖、批量 indicators、UI 交互/图表消费、诊断真实订单关联、K7 工具契约；这些均有独立调用链，不能用 A 表高层“已落地”一句话核销。
2. 对命名文档作交易语义反证：序列化不变是可在 serde 和 host wire 层复核的代码契约；但不能从实施命名记录得出现行 A 股合法性认证。本轮没有交易制度代码变更，也没有法源更新。
3. 对旧红灯的完整重核以目标提交新增验收档为依据：最终源码版本与目标源码相同，故失败现况已解决；对“当时执行失败”仍保留原始真实结果。其他当前 G39、公司实际 E2E/桌面拖拽等未完成矩阵仍按原报告标记，不从成功 Rust/Web 自动化中推断通过。
4. 无新增代码修复建议。历史审计覆盖计数、失败记录与后续验收可以按提交/时间读清楚；若正式文档要代表“当前验证状态”，建议另加最新验收链接，不覆写历史失败账本。

## 复核结果

- 大 A 语义：新增验收不改成交、账户、费用或 ADR-0025 存档规则；没有以通用默认冒充板块规则，也未核验或新增现行交易制度。
- 必要性：三份文档内容与 A/B 分类可追溯；未发现应把明确未来项提升为当前实现任务的理由。仅需保留 G 层细节及验证边界。
- 遗漏/边界：6 个命名批次旧失败已有后续完整验收覆盖，需读作已解决的历史失败；真实长验收/每 case 时限和 G39 不因此全部核销。
- 本文件仅新增于 `agents/implementation-audit/exhaustive-review/luna43.md`；未改产品或既有文档，未运行测试。
