# 测试先行：LibOS 析构与恢复

本批只改测试、验证夹具及文档。Red 来自真实可观察行为，不以声明缺失或无法编译充当行为证据；不改生产实现，也不把新用例标记为忽略。

| 边界 | 接口/语义断言 | 证据等级 |
| --- | --- | --- |
| WASM Engine Worker | `start_wasm_reclaimer()` 在 Memory、DEAD 与 substrate 已登记并被 supervisor 确认后调用一次，在业务 `ready` 前完成。`close_wasm_reclaimer() → bool` 的 true 表示队列完成与 join；false 或异常不产生退休成功。首次 create 失败处理已分配 Memory；消费 session 后仍保留两路失败信息。同 Memory 换代不消费 reclaimer，重复关闭不再次消费。 | `wasm-worker-ownership.test.ts` 驱动真实消息处理器、用受控 FFI 替身定位编排；不是运行时线程回收证明。 |
| LibOS 后台析构 | 正常容量的提交返回排队，消费者闸门未开时未实际析构；随后 FIFO 结束观察每个析构恰好一次。队列满后的内联回压与真实断连的诊断分开。浏览器的退出必须对应真实 Worker 与同一 slot 世代；退出和零资源 drain 均先于最终结果。 | Native 的真实有界队列测试；浏览器 `--deferred-contract` 在隔离 Chromium 加载共享内存 Release 验证制品，不接受把 Inline 误报为 Queued。 |
| 读档 | 自由重启校验实际存档、游标、日历和每批事实的合法性；分开记录两臂的 NPC 与交易事实。独立的受理事实重放入口必须捕获、注入并证明同一批实际 World 收据；缺能力时 fail closed。 | Native HTTP/WS 自由续跑实测；原 strict 事实诊断仍失败。两天完全闭市且无交易输入的 Rust 测试隔离稳定 Civil 事实 key 重置。 |

准入：修实现后要求上述 red 转绿，并重跑现有用例、真实 Browser Worker 退出和 Native/WASM 产品测试。通过编排替身测试不等于真实线程完成。封装开销另做同平台、同语义对照；两端采用不同的析构执行方式时不能互相代替。
