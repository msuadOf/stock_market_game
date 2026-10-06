# Rebase 完整证据构造证明（2026-10-06，在途）

## 目标与实现

固定真实Rust SessionActor Fastest，默认5股/20007NPC/seed42、Rayon16/Tokio2、16物理核0–15；frontend暂停，1000x仍未达成。f0ac profile全ledger未命中门禁7690次/2.627s，receipt应用2.564s，rebase3001次/0.244s，重叠不可相加。上一有序join139.45x比原版154.28x慢，已撤回；当前不恢复该join或页cache。

当前只在成功的rebase_private_for_tick_commit末尾向既有EvidenceCache安装完整成功证明：原完整输入gate不动、每个变化Envelope的rebase/validate不动、terminal审计/守恒行移除与非空守恒清零不动。已有全输入证明保证所有资源非负、每行/全局checked汇总合法；新basis=live，live没有增加且非零，spent/released=0、ConservationState=EMPTY，累积audit完全原样保留。删除terminal只减少总额，故新基准全局总额合法；审计/evidence结构对应仍成立。缓存只有Weak四map完整identity，无新持久字段、规则、默认值或第二套交易路径。后续mutation仍通过Arc::make_mut使identity失效。EvidenceCache::capture统一两处身份构造，锁失败显式InvariantViolation。公开rebase仍先clone candidate，因此失败不写回caller；private失败由整tick丢弃。

费用/T+1/报价过期资源分配/局部受理/撮合/时钟不变，依据沿用docs/trading-rules.md及ADR-0017/0018；不引入交易制度，不宣称重查官方费率。

## TDD与基准

新增active_tick_rebase_carries_complete_evidence_proof_into_the_next_read_only_gate真实红灯：来源已完整校验并成功清除terminal后，下一只读门禁仍触发1次完整扫描，要求0；当前绿色。fixture最初使用Sell key携带cash，虽现有Envelope.validate不禁止，已改成符合A股预留语义的Buy key，再执行绿色。随后ledger58短测通过，RAYON4/test4、9s TERM+1s KILL。

无构建/其他基准重叠，release-j8与debug-j8并行构建总job16，各299s deadline、已确认exit0（53.27s/42.03s）；candidate163.982x /3001tick /18.3008s，最近窗口116.96x；随后相邻独立f0ac baseline155.042x /3000tick /19.3496s，约5.77%收益。这仍不是>=1000x、满核或全交易日验收。

新增边界刚加入、正在重新编译：Seller逐腿封顶费用债保持、与独立owned正确ledger的Eq/成功hash_projection字节一致、四map mutation失效/branch authority隔离、i64MAX现金与u32MAX股份上限、全部terminal删除到空ledger。待绿色和全新一次性Luna完整窄diff独立复核后才报告完成/本地提交。frontend/WASM/HTTPWS/full workspace/full day未验收；原save_contract日历错误未改。

## 最终验证与独立复核

新增边界重新编译exit0（22.14s/-j16），最终ledger60/hash14/continuous tick19/auction transaction14四进程固定Rayon4/test4短测全部通过（各<=2.15s），filter重叠不累加唯一case。同SHA正式确认164.415x /3000tick /18.2465s，最近窗口113.80x，CPU样本均值4.254核（16核预算26.59%）。SHA a7a2b34cf8eccdb49eb11786e316d0c9385ba3da623aff55e9f71f6f4fc30c31。所有构建/基准handle已确认exit0，没有在途长任务。全新一次性Luna high独立窄diff复核通过，无阻塞；报告rebase-proof-independent-review.md，只静态审核，不冒充独立重跑。准备本地聚焦提交，不push。1000x仍未达成，frontend暂停。
