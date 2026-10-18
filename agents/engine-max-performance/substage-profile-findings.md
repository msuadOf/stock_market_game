# 真实 server 子步骤计时（2026-10-06）

在已审查 c06151ac 上临时插桩26个子步骤，保存18份原源码字节/hash；保持默认5股20007NPC seed42、16物理核affinity0–15、Rayon16/Tokio2、真实SessionActor Fastest及连续逐帧广播。插桩总运行23.459s /3001ticks，127.927x；仅作定位，不是正式吞吐验收。父子span与跨股票并行span有重叠，不可直接把所有数字相加。

| 子步骤 | 调用数 | wall time 累积秒 |
|---|---:|---:|
| SealAllocationSnapshot完整 |3001|3.286|
| 其中预留重新汇总 |3001|1.633|
| 其中account资源校验 |3001|0.516|
| Continuous inputs准备 |2401|2.775|
| Continuous shadows初始化 |2401|2.263|
| ledger完整未命中校验 |7690|2.636|
| ledger守恒 |13435|2.129|
| ledger并行row mapping |25540|2.429|
| hydrate live envelope |3001|2.407|
| project live envelope |3604|1.628|
| NPC排入下一tick |3002|2.120|
| 其中NPC snapshot |3002|1.827|
| 其中retail analysis |3002|0.905|
| Continuous shadows结束 |2401|1.219|
| 私有ledger完整校验（并股重叠） |12005|0.702|
| tick shadow clone |7189|0.669|
| CommitTick state交换 |3001|0.536|
| runtime snapshot |4185|0.120|

root已逐字节撤回插桩并保存proof在substage-profile-rollback-proof.json。脚本substage-profile-instrument.py、原module归档substage-profile-module.rs、临时源码patch substage-profile-rollback.patch均为工作文件，不进入生产engine模块或正式API。binary暂存.tmp/server-substage-profile-c06151ac，严禁把它当未插桩原版。

下一步实现 live reservation 的不可变page投影复用；密封资源仍使用ExpiryShadow后的live ledger。页以完整account ID范围分组，同账户不跨页；继续保留每账户逐行checked现金/股份运算、零股份未知stock拒绝、未知账户优先错误，以及mutation/terminal/rebase后的缓存失效。先短fixture红绿与全部输入失效检查，再真实server配对测试，收益不足则撤回。
