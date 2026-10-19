# W3 批（web 查询面接线）实现与核对记录

> 实现 agent 落档：2026-10-20。分支 `feat/web-reintegration-w3`（单父 fe9e2ff5）。
> 范围与验收锚点：`reintegration-campaign-plan.md` W3 节。

## 提交清单（8 件）

| 哈希 | 内容 |
|---|---|
| 5ccd7a4c | fix(engine) B2 补面——intraday_average 纯函数面恢复（Gap A，main 批准） |
| 582499ce | fix(engine) B2 补面——交割单游标分页方法恢复（Gap B，main 批准） |
| 28f459f4 | feat(engine) ProtocolSession 四查询面透传 + wasm 绑定 + 真出数测试 |
| 081b3acf | feat(web) G2/G6/G8 面板组件取回（素材件） |
| e24c349d | feat(web) host 归一层四查询面 + engine-host 视图类型族 + worker 传输 |
| e8b50036 | fix(web) G8 存档 schema 接入 B2 四字段（修复 roundtrip 断裂） |
| 08071fad | feat(web) 四面板接线（Provider/桌面/移动/交割单/VWAP） |
| （本文件） | docs 台账 |

## ADR-0041 Coded\* 词表逐点核对（W3 验收项）

| 核对点 | 结论 |
|---|---|
| 视图层寻址词表 | 全部保持六位代码串（`code`）——面板/模型/视图类型零数值 id 感知 |
| wire 词表 | B2 重建后 wire 为数值 `StockId`（请求对象 `stock`、确认行 `stock`）——与计划 W3 节"MarketHistory 系走六位 code 串不受影响"的预判相反（该预判基于旧 main wire；B2 实施选了数值 wire，符合 ADR-0041"wire 只传 id"），逐点适配完成 |
| 请求侧翻译 | `wireMarketHistoryRequest`/`wireCurrentMinuteHistoryRequest`/`wirePersonalTradeHistoryRequest`（host 边界 code→stockIdOf，未知代码显式抛错） |
| 响应侧翻译 | 归一层入口 `stockCodeOf`（stock-history.ts 既有先例同式），MarketHistoryPage/CurrentMinuteHistoryResponse/PersonalTradeConfirmation 三面 |
| PersonalTradeConfirmation 字段 | wire `stock/price_cents/gross_cents` → 视图 `code/price/gross`（引擎 B2 命名，非词表破坏） |
| 非引擎面 | 私有历史查询编排（private-history-query）、面板聚合（minute-kline/retained-history-model）纯前端，不碰 id 词表 |
| remote-membership/store | B3 批已完成 Coded\* 对齐，本批未触碰 |

## 与 B2 wire 的语义差异适配（非缺口，B2 契约即新基线）

1. `CurrentMinuteHistoryResponse.status: "Trading"|"Closed"` → `trading: boolean`（二值同构）。
2. `observed_at`（CivilInstant）不再随响应下发——实时"形成中"桶标记改由调用方以
   游戏时钟提供（`timeline.minuteSlot(tick)`，tick=0 显式 null），不从 bar 伪造观察时刻。
3. `MinuteBar` → `MinuteBarFact`：字段名/字符串口径逐一相同（仅类型名）。
4. 旧 `HistorySessionStatus` 折叠为 `RetainedMinuteSession.trading` 布尔。

## B2 面 gap 处置（main 已批准按 18725d07 基线恢复）

- **Gap A（VWAP 纯函数面）**：IntradayAverage 无 Serialize/TS 导出；curve/Input/
  parse_turnover_cents 缺失。恢复件 5ccd7a4c（一致性规则单点化，单点直算不经
  u64 会话域杜绝静默截断；基线测试全量重立）。
- **Gap B（交割单游标分页）**：personal_trade_confirmations_page(account,
  before_receipt)（降序/排他游标/100 条页）。恢复件 582499ce（含基线测试）。
- **B2 验收遗漏登记**（main 确认属 W3 G8 范围）：B2 存档四新字段未进 web 严格
  envelope → save→parseSaveSlot exact() 拒绝（roundtrip 实断）。修复件 e8b50036；
  已在 `b2-side-effects-ledger.md` 补记。

## 四面板出数验证（真实查询结果，非 mock）

Rust 侧 `apps/web-wasm/src/protocol_tests.rs::w3_query_surfaces_return_real_traded_data`
（native runtime 生产路径——真实 NPC 盘养出卖一、玩家对手价限价单真实跨价成交、
日结冻结保留链）：

- 当日分钟K：真实 bar `{minute_of_day:582, o/h/l/c:"900", vol:"100", 额:"90000", 笔:"1"}`；
- 交割单/个人历史：真实 receipt `{receipt_id:"29", Buy@900×100, 额 90000,
  fees{佣金500/印花0/过户1}}`，水位 30 续号正确；
- 保留历史：日结后 Traded + 冻结 bars 非空 + settled_through 推进。
- wire JSON 形态与 web 归一层测试 fixture 逐字段同构（stock 数值/trading 布尔/
  price_cents 命名）——TS 测试断言的即该真实 wire 词表。

**已上报 main 的运行期限制**：wasm32 串行管线不安装 B2 三数据面（分钟桶/保留链/
receipts 只在 runtime 驱动器路径）——web 产品内面板数据面待引擎侧串行路径补装
（发现1）；runtime 日K stats 与分钟桶存在漂移窗（发现2，占位K覆盖嫌疑）。两者
均引擎域，W3 不越权处理。

## 既有失败集（非本批引入，干净树复现）

1. `wasm-worker-ownership.test.ts` 两例（restore owner 流程，HostFailure 契约解析
   抛错——B3 合并后形态，与 W3 改动无关，stash 对照复现）。
2. `file-target-permissions.test.ts`（缺 apps/desktop Tauri 生成物 acl-manifests.json，
   环境性）。

## 验收数据

- `tsc -b`：32 错误=基线（唯一 diff 为 remote-host 两条基线错误行号平移）。
- web 全量测试：除上列既有 3 例外全绿（含新增 7 个 schema 测试、6 个 host 归一层
  测试、private-history 3 例、面板 SSR/交互测试）。
- `cargo test -p web-wasm --lib`：19/19；`cargo test -p engine --lib`（intraday/
  receipts/b2 契约过滤）：15/15。
- wasm32 目标 `cargo check -p web-wasm` 通过。

## 过程事故记录（诚实汇报）

批中段工作区遭外部并发 git 操作（reflog 见非本 agent 的 `reset: moving to HEAD`
与旧 stash apply），tracked web 修改与 staged 面板被清。恢复路径：引擎侧与
untracked 文件幸存先行提交；web 修改按对话内编辑记录逐文件重放；一次提交因
外来 staged 文件夹带立即 soft reset 修正为纯 pathspec。外来残留（退役公司系统
半套 modified/untracked，致引擎编译断）已定向 restore/clean——内容在
18725d07 与 stash@{0} 中可考，无数据损失。后续建议：本 worktree 排查并发
git 会话来源。
