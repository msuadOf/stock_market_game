# G29：零 NPC 开局

ByKind只有在至少一个NPC存在且正流通盘需要分配时才验证有效种类权重和；逐字段finite/非负检查始终执行。零NPC沿用seed_float空集合早退，不把流通盘送给玩家，也不定待决Q06分布政策。

2项新短case旧码1失败、1通过；修复后2通过（0.89s）。同case分别从Random、正权重ByKind、全零ByKind真实新局运行1tick、日结并恢复，账户只有玩家且无持仓。非法负权重/NaN/Infinity仍拒绝。另单独过滤现有有效种类零权重/只给缺类权重/权重和溢出3项反例，3通过（0.00s）。

构建`--no-run -j16`，执行`RAYON_NUM_THREADS=8 timeout 10s <session-binary> <filter> --test-threads=8`。不运行回归、长行情或浏览器验收。

非实施者restore_batch_review审查完整diff，确认没有赠股、放宽非法参数或触碰Q06，三项代码门禁通过；绿测与最终台账再次复核通过，没有新增发现。
