# std-thread 迁移阶段工作产物归档

> 2026-10-17 从 worktree `.tmp/`（gitignore，易失）归档入库。`.tmp/` 内仍是工作副本；
> 本目录为**长期引用版**，后续产物按同结构追加并更新本索引。

## plans/ — 计划与操作手册

| 文件 | 用途 |
|---|---|
| flip-runbook.md | 翻转批手册（前置检查/翻转步骤/双口径终测协议/Sol 终验收/回滚） |
| batch9-execution-plan.md | 批 9 外部清理 16 步执行计划（带行号） |
| batch9-external-cleanup-inventory.md | 批 9 摸底（server/web/旧引擎删除候选） |
| wasm-gap-verification.md | wasm 缺口核实（25 错误实锤+方向 A 裁决依据） |
| maker-taker-consumer-scan.md | Trade maker/taker 语义变更消费面清单（30 处） |
| semantic-traceability.md | trading-rules 39 测试面追溯矩阵（#103 菜单） |
| dying-tests-census.md | 死亡测试清点+27 条再锚定断言（#103 菜单） |
| assertion-strength-audit.md | 断言强度六模式审计（33 命中/11 高置信，#103 菜单） |
| intentional-divergences.md | 有意分歧/登记缺口/pin 锚三类总账（Sol 终验收+批 10 用） |
| adr-draft-std-thread-runtime.md | 运行时模型 ADR 草案（批 10 入库） |
| disk-worktree-inventory.md | 磁盘/worktree 盘点（任务 #63 前置） |

## reviews/ — Sol 复核提示词与判决（复核史）

sol-*-review-prompt.txt（各轮送审提示词）。终裁原文见 reviews/ 下同名 verdict
（sol-flip-review-verdict / sol-recheck-verdict / sol-mid-d3-verdict /
sol-mid-d3-recheck-verdict / sol-third-d3-verdict 在 plans/ 或 ledgers/）。

## handoffs/ — 实现轮换交接（第 6→11 任）

## ledgers/ — 主台账

multi-tick-design.md（D0-D5 执行记录全链+A/B 拍板）/ d5-triage.md（逐域判型）/
d-batch-plan-v2 / flip-106 清单族。

## evidence/ — 红绿证据与环境数字

sol-mid-d3-red-evidence-r3/（MANIFEST 含命令+SHA）/ sol3-red-evidence/ /
env1-*.txt、baseline-*、after-*（106→6 各阶段失败名单审计链）。

## design-docs/migration-design/ — 方案设计

A-F 收敛架构（F-converged-architecture.md 为权威）+ G 批计划。
