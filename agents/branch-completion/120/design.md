# #120 日末 purse 与失败撤退设计（2026-10-25 起，设计已复核，最终实现待终审）

## 已冻结证据

- 基线 f5820291；本树 HEAD 626f0626；生产 desk 尚未改动。
- 3799c401：真实两组线程短红证；626f0626：终态失败统一 join 后耗尽在途上行，无次生清理异常时保原 WorkerFailed，有次生异常则在ProtocolInvariant保留原错与完整清理上下文。
- 原 JSON sha256 `48f04c54be6ce5adf16d47192689dc318c7a7aa010303d1a2504e6106256ff52`。
- 新构建 release 在原输入 seed42 / groups2 / 100tick 上仍失败：tick90 stock2 owner(0,25)，purse459205、required551006。确切输入和原日志在主树同目录。

## 时间归属真值表

以 tpd=30、opening=8、closing=4 为例。`run_ticks` 的 tick=N 是完成第 N 个窗口；委托受理窗口为 `phase_at(N-1)`。这是现有 place、日末 Drain、driver civil 时钟共同契约，不重定义序号。

| 步骤 | 本次委托归属 | 当前日涨跌停基准 | 收盘清算 | 行情归档/重置 |
|---|---|---|---|---|
| Start89 / 任意 Continue(89,r) | 第三日收盘集合竞价（索引88） | 第三日前收1274 | 尚不执行 | 尚不执行 |
| Start90 / 任意 Continue(90,r) | 第三日最后收盘申报窗口（索引89） | 仍1274；20%涨停1529 | 尚不执行 | 尚不执行 |
| 成功日末 Drain90 | 第三日结束 | 清算沿用第三日前收1274 | 清算全部已受理委托 | 清算完成后，归档终K、昨收置真实收盘、量额桶重置 |
| Start91 / 任意 Continue(91,r) | 第四日首窗口（索引90） | 第四日前收=第三日真实收盘1529；20%涨停1835 | 按既有开盘阶段规则 | 不再执行第二次roll |
| 手动 Drain89→Start90 | 手动清当前簿；tick90仍属第三日 | 仍1274 | 沿用现有手动清算行为，但不得提前改昨收 | 不做自然日归档/roll |
| Drain90失败→AbortDayEnd→重试Drain90 | 已提交tick90始终保留 | 回到Drain前1274；重试从同一事实重算 | 第一次副作用全部冲回 | 归档/昨收/桶由现有DrainMarketSnapshot回滚 |
| Drain90成功后shutdown再次Drain90 | 无新委托、同一个已完成Drain | 保成功后的新昨收 | 不重复清算 | 不重复归档/roll |

数字为独立会计推导：1529×300+500佣金+5过户费=459205；1835×300+500+6=551006。测试用 stock2 / owner(0,25) 保留历史坐标。

## A股依据与已有简化

本次适用日期2026-10-25，核对当前官方2026交易规则：深交所3.3.14按前收盘价计算涨跌幅；5.2.3将即时行情前收盘价定义为上一交易日收盘价；3.4.3真实盘中/收盘竞价最终距离参考最近成交价。本次仅维持已登记的统一昨收与无收盘成交时最后成交价简化，不宣称实现全部真实收盘价制度。
官方原件主树 `agents/branch-completion/116/szse.pdf` / `szse.txt`，来源：
https://docs.static.szse.cn/www/lawrules/rule/trade/current/W020260424690713155663.pdf

`docs/trading-rules.md` 已明确所有深市竞价统一昨收为简化，同距取低价为模拟确定性。本案不改变这项政策；必须保真正前一交易日昨收至收盘清算完成。反例：旧昨收1000、日内last1100、收盘buy1100×100/sell900×100；按既有简化候选900/1100同距选900；若roll先改last_close=1100就错误选1100。

## 最小生产落点（提请复核）

1. `desk.begin_tick` 移除提前 `roll_day`。tick90受理全轮都沿用旧昨收。
2. `DeskMsg::Drain` 在现有 `snapshot_drain_market` 后先 `drain_all`，完成清算后仅当 `cur_tick>0 && cur_tick%tpd==0` 才归档终K和执行 `roll_day`。终K/额/笔取清算后事实，沿用现有输出字段。
3. 不新增marker：同tick是否已执行Drain直接由现有undo里 `DrainMarket` 记录判定。第二次Drain只发当前诊断应答，不重跑清算、归档、finish或资源flush。Abort消耗undo，故重试重新执行；下一Start清undo，故后续tick可Drain。
4. `build_drained_public` 直接使用Drain后真实 `market.last_close` + next_tick的相位/cage；去掉基于tick除法的提前昨收预测。保持#125的band helper与sell基准修复（最终由root在新main树整合复核）。
5. 可信设计复核同意退役只为提前roll补救的unroll/reroll、`rolled_day_candle`与无消费者的`rolled_day_stats`；保留`rolled_day_bars`作为Drain应答完整分钟桶提取口。
6. 不改费率、开盘相位、价格笼子、T+1、外部API、存档格式或深市竞价简化政策。

## 验证

- 原数字短红f056655b已在实现前真实失败；最终原数字、多轮、板块双向、真实深市tie与量额笔测试均通过。
- 覆盖tick90/91、两组、20%与10%、收盘向上/向下、最后tick多轮Continue、日结失败/Abort重试、重复shutdown Drain不重复归档，以及上述深市选价反例。
- 修正旧#121手动Drain239→Start240断言为真时标：保#121“广播=真实受理”的强断言，不保错误提前新日预期。
- 运行日结/分钟K/保留历史/费用/Seal与#121/125相关独立短套件，普通单命令与case均10秒；所有构建外deadline299秒、cargo -j16。
- 原15007 NPC规模不能替代其历史精确重测；单列历史长验收，外监督共享300000ms，groups2 + stocks5，采样线程CPU，保留失败与清理证据。不会以短fixture绿冒称原场景绿。
