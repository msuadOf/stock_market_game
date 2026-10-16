# D 批划分 v2（基于 P1 整改后清单 .tmp/flip-failures-after-p1.txt）

日期：2026-10-15 晚。顺序 D2→D4→D3→D1→D5 不变，边界按新清单调整。

| 批 | 域 | 项数 | 内容 |
|---|---|---|---|
| D2 | t3t4(12)+attention(3)+expiry(3)+npc_prep(2) | 20 | tick 生产契约/相位门禁/到期时序/玩家意图路由 |
| D4 | binding(17)+hash/poison(3) | 20 | 观测/绑定+hash 契约 |
| D3 | candidate(17)+envelope(6)+settlement(1) | 24 | 翻转核心缺口 |
| D1 | decision(29)+plan(6) | 35 | 决策链状态机+计划域 |
| D5 | save(7)+civil(1) | 8 | 存档内容（依赖前四批） |

新增 7 项归域：authoritative_tick×4→D3（candidate 域同域）；typed_private_plan→D1（plan）；decision_observations→D1；stock_output_reordering→D4。与原批无重复。

关键真缺口：玩家意图路由（D2 核心，attention×3 依赖）；candidate 投影×4（D3 内）；决策链状态机（D1）。
