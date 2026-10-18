# main 线公司系统退役清单（std-thread 合并 A 终态）

> 生成背景：2026-10-18 main←exp/std-thread-model 合并，用户裁决 **A 终态（分支主体）**——
> "以 std-thread 线为准，整体架构、代码组织；main 线只是保留分叉点后续新增功能局部模块，
> 比如公司子模块的建模"。合并提交树上 main 分叉点（a6001494，2026-10-06）之后的全部
> 公司系统实现从工作树退役；git 历史（main 提交链，尾端 `18725d07`）完整保留。
> 本清单是后续"公司系统在 std-thread 架构上重集成"战役的素材索引。
>
> 取回锚点：main 侧终态提交 `18725d07`（合并前 main HEAD）；分叉点 `a6001494`。
> 完整 403 文件清单：[`retired-files.txt`](retired-files.txt)（每行一个路径）。

## 退役范围总览

| 域 | 文件数 | 说明 |
|---|---|---|
| 引擎面（packages/engine） | 116 | 公司模块+Simple 会话接线+ROE/披露账务 |
| Web 面（apps/web） | 269 | 公司组件/存档 schema/fixtures/生成类型+K线/均线/远程登录/归档等支撑面 |
| Server 面（apps/server） | 12 | 身份路由/归档路由/公司域契约测试 |
| Desktop 面（apps/desktop） | 1 | 公司查询 actor 测试 |
| native-store（packages/native-store） | 5 | 身份/归档原生存储 |
| 公共文件 main 侧接线 | 324 文件的 diff | 见 §公共文件接线面 |

澄清一个易混点：Web 面的 K线周期/分钟K线/均线设置/远程登录/成交确认表/归档管理等
表面像"独立功能线"，经提交链核实**全部由公司系统批次带入**（a001681d"接通 Simple
新局、严格存档与共同财务查询界面"、f1bc6f84"接通共享账户、日终存储与共同公司查询
宿主"、ce5b3a1c"接入 Simple 会话、精确账户契约与日级历史"），是公司系统的支撑面，
不存在独立于公司线的第二条 main 功能线。

## 公司子模块建模（用户点名保留价值，重集成优先素材）

用户原话点名"公司子模块的建模"为主要保留价值。核心建模文件与提交链：

| 子模块 | 核心文件（packages/engine/src/company/ 下） | 主要提交 |
|---|---|---|
| Simple 四周期与年化 | `simple/`（周期生成、精确年化） | 7fb4aec2、8d07a779、e488cbfe |
| Simple 会话与精确账户 | `opening.rs`、`assembly.rs` 及 session 接线 | ce5b3a1c |
| 现金分红状态机 | `cash_dividend.rs`+`cash_dividend/` | 582720b1、e739ca8d（原子结算+同日除息） |
| 分红公告与年报基线 | `dividend.rs`、`dividend_contract_tests.rs` | 73949cfd、41eae110 |
| 法定披露 ROE | `accounting/disclosure_roe*`、`period_roe*`、`reports/roe*` | ebfb9f31、a86c7a19（扣非+同一控制） |
| 送转登记与碎股 | `stock_distribution.rs`、`stock_distribution_tests.rs` | fba94e8b、ceaeda2b、b804de69 |
| **送转个税（个人差别化+持有期扣缴）** | `cash_dividend_tax.rs`+`cash_dividend_tax/` | 02d68eda、78a35747、f1fc21f6、**c1561faf**（终态） |
| 配股/增发 | `rights*`（权证分配/认购/结算状态机/除权公式） | 5b01cb5e、7bb2d302、4467f085 |
| 回购 | `issuer_repurchase.rs`+tests | b2aa7fe1、6579e641、b804de69（专户事实） |
| 三层税制 | `income_tax.rs`、session 接线 | 389f2fc2、d4d68ef2 |
| 逐项分录 | `reports/simple_summary.rs` 及 Books 接线 | 03fe95e4、51186cde、c9848aa5 |
| 玩家提案 | session `player_proposals.rs` | d128ea36、c0d54e33 |

配套领域文档（决策记录，**未退役、仍在树**）：`docs/decisions/0037-company-action-initiator.md`、
`0038-simple-issuer-synthetic-settlement-funds.md`、`0039-rights-placement-split-scope-and-toggle.md`、
`0040-new-game-default-taxation-modes.md`。台账归档在 `agents/company-system/`（未退役）。

## 取回命令

按域整体取回（在重集成 worktree 内执行，不要直接在 main checkout）：

```bash
# 引擎公司模块与会话接线（116 文件全清单见 retired-files.txt 的 packages/engine 前缀）
git checkout 18725d07 -- $(grep '^packages/engine' agents/main-line-reintegration/retired-files.txt)

# Web 公司+支撑面（269）
git checkout 18725d07 -- $(grep '^apps/web' agents/main-line-reintegration/retired-files.txt)

# Server/Desktop/native-store（18）
git checkout 18725d07 -- $(grep -E '^(apps/server|apps/desktop|packages/native-store)' agents/main-line-reintegration/retired-files.txt)
```

提交链全量浏览：

```bash
git log --oneline a6001494..18725d07            # main 分叉后全部 ~200 提交
git log --oneline a6001494..18725d07 -- packages/engine/src/company   # 公司域 45 提交
```

## 公共文件接线面（无独立文件，取回靠 diff）

main 侧对 324 个公共文件（mod 声明、协议导出、session 管线接线、web App 路由、
CI 等）的公司线接线改动，随合并取分支侧一并退役。重集成时需重新对接（不能直接
checkout——会覆盖 std-thread 新架构）。取回参考：

```bash
git diff MERGE_HEAD 18725d07 -- <公共文件路径>   # main 侧接线的精确 diff
```

已知主要接线面：`packages/engine/src/lib.rs`（simple_company_test_fixture、公司导出）、
`packages/engine/src/session/mod.rs` 及 `pipeline/mod.rs`（公司域 mod 与日结接线）、
`apps/web/src/host/protocol/*`（公司查询协议）、`apps/server/src/routes.rs`。

## 语义与测试基线注意

- 分支侧终验收（lib 双模式 1661/0/11）是**不含公司系统**的树；重集成后测试数必然
  增加，需另立基线，不得沿用 1661。
- main 侧公司域测试基线证据在 `agents/company-system/`（如 `implementation-checklist.md`、
  三宿主契约矩阵 handoff）。
- 分支历史注记：42ac1d9b（2026-10-07）曾把公司线 221 文件带入分支做 CompanyId 数值化，
  随后 87f6f7d9/84c2477c 以"误入的外来文件残留"清理——重集成时该批数值化适配可参考。
