# 未保留的 ledger 迭代器长度实验

最新48组server调用栈中actor多次在收集完整ledger row。原paged迭代器size_hint为(0,None)，没有利用map维护的len；新增65行跨page/group、正反混合读取/耗尽的短测试，真实红灯后用CountedRows传递精确剩余数，避免collect扩容和count全表遍历。相关ledger57、private6、adapter短测通过。

固定真实server、默认5股20007NPC seed42、16物理核affinity0–15、Rayon16/Tokio2，无构建/其他基准重叠的相邻配对：candidate126.330x，已审查HEADc06151ac原版128.505x。没有总吞吐收益，全部wrapper和新测试已逐字节回退HEAD，不进入生产diff，不另开形式review。原版binary .tmp/server-private-stock-c06151ac SHA256 5ea3e836987961844f8852ee55722796fe29c4aefcc1ce5a940640904fc52dcb。

当前 .tmp/perf-target/release/examples/server_max_speed_probe 以及lib test binary仍含已撤回wrapper实验，下一轮验收必须重新编译当前源码或明确使用上面的已审查原版独立binary，不能拿旧mtime/相同文件名替代源码绑定证据。


## worker 调度对照

已审查c06151ac同一原版binary、相同16核affinity，只把Rayon16改为Rayon1：真实server119.186x /25.170769s；16worker为128.505x。16worker吞吐仅提高7.82%，实际使用约4核，而非线性增长。单worker实验只校准调度与计算成本，刻意串行是实验控制变量，不能作为达标或生产配置。不能把资源空闲解释成quota，也不能用busy wait凑CPU。后续优先检查逐阶段外部caller进入Rayon的屏障开销与全量状态投影成本，仍须保留独立NPC结果及时消费、并发session不饥饿、完整tick原子性等已有合同。


回退后的debug/release已分别真实重编并exit0，恢复了当前主路径与已审查源码的绑定。没有在构建期间运行性能基准。
