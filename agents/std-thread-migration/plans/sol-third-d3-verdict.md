# Sol D3a 三审终裁（2026-10-16，范围 798455d6..531c6c15）

**终裁：不通过（接近闭合）。** 真实修复：existing 时点/日结+ForceDue/身份关联
主体。可以继续准备 D3b，但不得以"D3a 已闭合"为前提。

## 六项判定

| 项 | 判定 |
|---|---|
| 1 existing 时点 | 真实修复（单测不能检测调用点再次后移——时序靠代码审查保证） |
| 2 余量+live purse | **部分**：卖票成立；**买单读面=预留下界，漏 desk 持有的价优差额** |
| 3 身份关联+逐轮转发 | 主体真实；未受理票（order_id=None）Closed/拒收关联边界缺证 |
| 4 日结+ForceDue | 真实修复 |
| 5 首步前注入 | **部分**：seed 构造期转交✓；**首步前 queue 仍走旧 staged 通道**（npc_tick_preparation.rs:707），05b 测的是 driver 在位路径不证明首步前 |
| 6 断言补强 | **部分**：受理非空✓；部分成交断言可被零成交腿满足（drive_tests.rs:1088 只查 remaining=Some）；terminal 未覆盖 |

## 买单钱包反例（必须修）

限价 1000 分挂 200 股、100 股以 900 分成交：
初始 purse=200502；实际支出=90000+500+1=90501；desk 真实余额=**110001**；
新组读面=**100001**；漏计 **10000**（价优差额仍在 desk）。
02 红证 right=100001 只证明"旧全量加回已改"，没证明 live purse 正确；
测试预期复用实现同款费用 helper ≠ desk 余额对账。须区分总现金/在途现金/
可分配预算，避免冻结资金重复使用。

## 报价触发时序：裁定 (a)

多拍+明确 force 的 fixture 适配。理由：新局簿为空；ADR-0025 公共恢复入口
只收日终档、拒绝日内活动委托——无生产证据需要"首拍决策见迁移旧单"。
不为 fixture 增加 (b) 生产 Continue 报价触发面。

## 三项登记裁定

- item1 降级：接受（但只能称代码时序审查+单测，不能称端到端红绿）
- item5 否决全局 env 分流：接受（112 项连坐实测），但不豁免首步前 queue 缺口
- item6 terminal 登记：接受"尚未覆盖"，不接受据此验收——小型确定性
  fixture 即可验证 terminal 投影，无须等巨量成交

## r3 红证裁定

无空文件（进步），但"全部含断言原文+left/right"**仍属实失实**：
01 不存在于 r3；03a/03bc/05/05b 缺 left/right。缺逐份命令与源码 SHA，
不能认证全部同源性。terminal"实测红"原始日志未见。

## 剩余最小修复集（四项）

1. 按 desk 实际占用维护 live purse：价优部分成交跨层对账（desk 余额==组
   读面）+Abort 验证
2. 接通首步前 queue：handover 恰一次消费+attention 不重复推进的证明
3. lifecycle 部分成交断言绑定同一 child（0<remaining<child_qty+匹配独立
   成交事实）；补最后计划 terminal 覆盖旧 Active 镜像测试
4. 红证补齐（实际值/命令/SHA）；身份边界补测（order_id=None 的
   Closed/拒收关联），发现错误则修

另：chain_restructure_tests 最终日志 26 项失败——"28 例族"名单要明确到条。
env 基线 55→最终 54（减少项=external_host_consumes_npc_results_on_its_calling_thread）。
