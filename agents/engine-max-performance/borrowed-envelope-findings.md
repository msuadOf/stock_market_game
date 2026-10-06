# 借用 Envelope 投影优化记录（2026-10-06，在途）

## 目标与范围

继续固定真实 Rust SessionActor Fastest，默认5股、20007NPC、seed42，Rayon16/Tokio2、共同16物理核affinity0–15。frontend暂停；1000x未达标。依照此前substage profile，优化每tick hydration与Continuous adapter的重复owned Envelope/StockCode分配。没有改撮合、受理顺序、费用、T+1、时间或默认负载；A股与游戏简化语义沿用 docs/trading-rules.md、ADR-0017/0018。此批不引入交易制度。

SharedRows<Envelope>与仅charged setter两项前序实验均已撤回：对照140.547x；共享候选140.271x，setter140.960x，未获值得保留的收益；原patch/恢复hash证据保留。不把这些撤回实验算成生产成果。

## 实现

BorrowedTickStartEnvelope借用StockCode，独立计算原有live/audit，完整身份排序与去重。只有ledger已经非空时用只读借用验证；空ledger继续走原owned hydration。所有投影费用计算先完成，连续簿按旧seq顺序计算、随后按完整EnvelopeKey顺序检查nominal/charged；缺失身份暂时继续但最终完整比较拒绝，不改变首错合同。terminal重叠先于长度与字段比较，map key与Envelope自身key都检查。matches显式解构全部Envelope字段，新字段必须同步；严格保留origin/basis/live/spent/released/audit/pending_price与构造出的TickStart一致。

一般EnvelopeLedger::hydrate_or_validate公开接口与其owned契约保持不变。借用门禁只读，成功不改缓存/序列化/哈希/receipt cursor。scratch key通过String::clone_from复用缓冲。Continuous adapter调用同一费用数学函数的Copy facts，继续算nominal及验证错误；仍克隆完整权威Envelope作为独立stock snapshot，不引入共享payload。单股去重集合使用(account,order,side-rank)，stock在该循环恒定，因此身份范围等价；private market validation只复用lookup key，原全部校验保留。

## TDD与验证

真实红灯1：已有ledger的只读校验构造2个owned Envelope，要求0。真实红灯2：adapter只读取资源仍构造1个owned投影，要求0；当前均绿色。第一个初稿fixture错误地要求复合map成功serde_json，因既有key序列化契约报错；改为已有Eq/business hash，随后才取得预期计数红灯，没有篡改业务断言。

borrowed matches逐项改变全部24组字段，与owned完整Envelope Eq等价；borrowed ledger门禁与owned门禁比较成功、资源/audit漂移、缺失/额外/重复/空投影、terminal冲突及map key/Envelope key漂移的确切Result。原hydration费用债、rounding、nominal漂移、missing seller history、失败原子性短测通过。

普通命令9s TERM + 1s KILL。一次未限制默认Rayon总线程数的宽continuous filter在9s被终止，不算通过；之后按独立套件四进程并行，每进程RAYON_NUM_THREADS=4、test-threads=4：matching27、incremental18、tick transaction19、finalizer11全部通过，每命令<=3.2s。其余定向：hydration11、borrowed2、ledger state25、adapter10通过；filter有重叠不累加唯一case数。最终source binding编译后还需最新短测与配对。

## 已完成的正式测量（无构建/其他基准重叠）

- HEAD8d5ed41b独立binary：140.547x /3000tick /21.3452s，SHA bf08dda9941d3b66a82cc4b457b652807eb1bed73a5e1eae17c3f463503fbbb5；CPU样本均值4.218核/26.36%预算。
- 仅借用hydration：148.375x /3001tick /20.2257s，SHA e2025905d87b57b3daace848dd61d85c8b4514a2a15399483c75aeca3fbe870e；均值4.316核。
- 追加Copy facts adapter与scratch key：149.269x /3001tick /20.1046s，最近窗口103.20x。仍未1000x、未满核；单次不能宣称稳态。

前端、WASM、HTTP/WS序列化、完整workspace和全交易日验收未完成，既有save_contract日历失败未修改。完整生产diff待全新独立review；不推送。

最终source binding：debug/release各-j8、总16job、独立target和299s deadline，均已确认exit0（17.32s/53.79s），不是只根据Compiling日志认定。最新hyd11/borrowed2/adapter10/ledger25四进程独立短测均exit0。

最终相邻配对：HEAD139.2775x /3000tick /21.5397s，candidate155.4489x /3000tick /19.2989s，最近窗口108.02x。此前候选148.38x/149.27x，测量有波动，不挑最高样本宣称稳定性能。正在对最终candidate作一次确认。

最终同SHA确认：153.1472x /3001tick /19.5955s，最近窗口105.16x，CPU样本均值4.309核；最终SHA 5a51e9122c9bf1c6e13431bd8bcaad28b18dae0cf484068d983d543642858306。最终源码两次153.15x/155.45x，之前artifact约148–149x也保留，不删除慢样本。所有基准handle已确认exit0。

全新Sol独立审核完整9文件diff通过，无阻塞；报告borrowed-projection-independent-review.md。唯一可选建议为混合计算/charged/terminal错误fixture，未补，作为覆盖限制登记，不宣称首错组合穷尽。最新hash_contract14、quote_expiry12、stock execution6短测通过；ledger_tests filter匹配0条，不算测试证据。已有ledger_state25与borrowed门禁比较仍有效。

全新Sol独立审核完整9文件diff通过，无阻塞；报告borrowed-projection-independent-review.md。唯一可选建议为混合计算/charged/terminal错误fixture，未补，作为覆盖限制登记，不宣称首错组合穷尽。最新hash_contract14、quote_expiry12、stock execution6短测通过；ledger_tests filter匹配0条，不算测试证据。已有ledger_state25与borrowed门禁比较仍有效。
