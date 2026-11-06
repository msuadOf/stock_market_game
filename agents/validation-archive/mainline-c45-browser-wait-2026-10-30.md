# 2026-10-30 主线与真实 Browser 等待瓶颈摘要

任务仍未完成。交付顺序：engine/#122 → 宿主与 Browser → 同条件性能 → 全仓回归 → 整树独审 → 本地 main。

## 已交付与范围

- 唯一开发主线当前 `f0a76458df1684e4556ea82d15962b7f5a0c244f`；本地 main 仍 `bc8a2addb547c188f965faa57123fd59922dc0c1`，产品未推送。
- C 回滚保护与真实本次 prepare 来源分类：45/45 exact 通过，`84450/WAIT0/bc2053`，120336 ms。原独立 Sol 完整 source/actual 均准；原 `24ac6580` 已接 dev `d675d365`。
- C actual front SHA `6366776b99500fd88d779bbf7d5e7d8a269fb4c14582a7075ce5428c4cf5b8c0`。
- 严格 F2 marker 已接 `bb9037c6`；正常 App 返回入口已接 `f0a76458`，类型检查通过。后端生产分类闭环未完成，新按钮 Browser 未验。按钮文件 lint 仍报三处 diff 外既有 hook warning，不称全绿。
- 固定旧 `0adec3f5` 的正式全 Web 为1918/1918；不能代替当前组合全仓回归。

## 真实失败保留

freshX01 `79426/WAIT1/ae356e`：fresh Rust/pkg/metadata/TS/Vite/checker 七步过，但未完成15300拍，无 X、未恢复。复用产物的 retainedX02 `45877/WAIT1/dc6bc7`：约286942 ms，仅连续1..2470、Continuous、civil=null，8.608 tick/s，无 X、未恢复。不能再归咎编译挤占。两批均正常停止清场。

H1-B `74064/WAIT1/c69570`：15精准目标红，另2WS请求前件失败，不称17全目标红。后续窄WS批又发现空响应被当JSON解析；原失败保留，正在一次核齐完整HTTP/WS契约。15域主体修复与独立审查继续。

## 短 Browser 真实对照

`69591/WAIT0/8561d1`。同一旧0adec Rust/pkg，1股4NPC、15300日长，两个新 World；仅 coarseUs 100000→500，非产品默认修复交付。

| 项目 | 100000 us | 500 us |
| --- | --- | --- |
| 连续交付 | tick1..32 | tick1..32 |
| 31间隔窗口 | 3554.170 ms | 66.020 ms |
| consumer tick/s | 8.722 | 469.555 |
| start至完整finish | 4563.430 ms | 156.945 ms |
| pause后真实边界 | tick33/day0 | tick33/day0 |
| 最终 bytes | 75759616 | 75825152 |

两臂真实 owned2/joined2、groupFinal1/deskFinal1，四清场与 ECHILD 真。原未实施 Sol 有限 actual 准，root亲读原结果。源码链：普通每拍 collect_plans → Phase::Drain → try_recv Empty → coarse sleep；消息到达无法提前唤醒 sleep。最小产品策略修复另行 TDD/独审。

- actual owner receipt SHA `54d588002ccc7b798453b60d81b315c509b823dcc6613e0572b8ee53fe94d3dd`
- Browser result SHA `f40ebc680486d045101d3137ebaa3ee242eafca231c8a0f5161bea42331168c2`
- 清场 front SHA `ac1c9ba9be31b9f7b955f0ea5f10b477b969a03d4688302af3e430b77c28b154`

该32tick诊断不是完整日结、默认20007 NPC性能、1000倍目标或20局成功证明。

## 仍需完成

用户保留P/A各20个成功生命周期要求。旧2GiB总账与单局1GiB不兼容；新工程候选总账3GiB、单局仍1GiB、阈值512MiB，真实drop/carry/未知终值锁门不变。较大实测final82509824 bytes下20F+C=2597.75MiB，仍须实际20局验证。Luna覆盖旧红测日志且最终3GiB缺先行TDD原件，已禁止回填；事后基底反证只记负控，不称完整TDD。

完整日结X/restore、真实App换档与F5、P/A20、最终同源五轮ABBA/N2N、正式all和本地main尚未闭合。只上传本小文本，不上传大型产物，不推产品main。
