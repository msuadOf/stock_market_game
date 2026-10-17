
---

## 归档说明（2026-10-17 P2 三件落库）

提交对应：
- 4d2ebd6c（P2-1a 个人态安装 dirty 掩码）→ p2-1a-red/green.txt
- 821a5b27（P2-1b 账户回写事实 dirty）→ p2-1b-red/green.txt
- 0a9379dd（P2-2a EchoBelief 稀疏上行+增量缓存）→ p2-2a-red/green.txt
P0 分解：perf-p2-p0-breakdown.md（B 口径驱动器 72.8% 满负荷、深拷贝安装
链 ≈58%；P2-1ab 后复采样：组线程 59.6%——组侧全量上行 clone 成新大头）。

B 口径链条（300 tick groups=1，events 全程逐位一致）：
4.3（基线）→ 5.02（P1-C）→ ≈10.1（P2-1a）→ 10.39（P2-1b）→ **65.6
（P2-2a，+530%）**；1000 tick 53.0（事件密度 3.3×的自然衰减）。
A 口径不受影响（P2 改动均在 session/driver 投影层）。
