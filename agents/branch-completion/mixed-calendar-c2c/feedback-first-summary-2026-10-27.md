# V4 首片两项实际行为结果

2026-10-27。只新增 `mixed_calendar_c2c_feedback.rs`，原九项测试与helper保持原SHA，中央生产未改。

- 使用root授权6ac normal engine rlib SHA `9bee5bf8bb46234eaaeb075f82d38dde14ae2c4e9169664669f1627ed88f1480`，不冒新staging或本树纯API的整crate构建。
- source SHA `68a75d735ae39465c859a66ce9ad5a472a2b4e7c6d6e8cb6e945afc0b313bb8f`；helper仍 `d1120a2ad62e3d0c1ca15ec1126ec44cf2050b48c4f84cb07e4dafbb5f4b2eb3`。
- rustc codegen8/lld8真实编译exit0、0.852922秒；binary `.tmp/mixed-calendar-c2c/c2c-feedback-first-6ac`，49320304 bytes，SHA `6c6e5486eb8fe72eab560da5aebc4fc698049abbf09e9b5a30d54cd9e1f7a5f1`。
- 先独立 `open_limit_is_accepted_and_reserves_real_cash`：wrapper exit0、0.809918秒，真实OrderAccepted、无拒绝、挂单预留现金正控全过。
- 随后两case以2个独立进程并发：open exit0、0.815504秒；`closed_limit_returns_strict_exchange_closed_feedback` wrapper exit1、0.994848秒。
- closed首断言为测试第67行：`left=[]`、`right=[String("ExchangeClosed")]`。原stderr另含最内层 `bounded command exited with 101`，JSON诚实记deadline CLI wrapper1，不冒直接raw子退出码。

实际源码编译前后/运行前后SHA相同；compile/run计划与完整stdout/stderr/result在 `feedback-first-red/`。每case与整批均由绝对Node和main树v3监督器10000ms限制，case libtest1线程，2process并发。运行结束无在途case。

旧 `mixed_calendar_feedback.rs` 是此前已编译、尚未运行的全矩阵准备，本首片不执行或改写它；交由协调者安排的fresh reviewer/runner接力。没有把编译成功或准备源码冒成该矩阵已红/绿。本片等待未实施者独审，不宣称C2-C生产闭合。
