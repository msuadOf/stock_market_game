# LibOS 公开接口补测：v6

这批补丁叠加在 v5。只含 Unit Test、真实浏览器验证夹具、性能对照和 `verification-harness` 专用 Rust 接线；不改变发布包、市场行为或存档。未编译或执行。本表描述代码已写的断言，不是通过报告。

| 接口 | 测试断言 | 入口 |
| --- | --- | --- |
| `ShutdownWake` | 无登记时断连、部分登记先唤醒后断连、重复不送额外哨兵、未移交区分无资源／有清理义务；Native 真实线程看到停止标记，附加清理诊断不覆盖原 panic | `packages/libos/tests/construction_cleanup_contract.rs`；两宿主共用 `tests/support/construction_cleanup.rs` |
| `init_wasm_wait` | 未初始化不能收件或公布布局，非法配置后可重试，重复初始化不能改先前设置或布局，不丢已缓冲数据 | `scripts/fixtures/libos-wait-api-browser.mjs` 的独立 Memory 案例 |
| `WorkerHandle::join` 与退出 | 结果被消费时真实子 Worker 仍可卡在销毁闸门；正常开门后要看到真实销毁、退出和槽回收；强制终止不能当成功 | `agents/wasm-tombstone-followup/browser-fixture/` 的 join 生命周期夹具 |
| `defer` 与 owner | 在真实浏览器中读取现有生产提交结果和实际析构计数。饱和／关停／故障合同保持严格；同步 `Inline`、缺少 owner 或缺失实证不可算通过 | 同一 browser fixture 的 `--deferred-lifecycle-contract`；Native 队列断言仍在 `deferred_reclamation_contract.rs` |
| Browser 性能 | 同一 WASM / supervisor 底座进行 API 与直连的交错对照，不把 join 取结果、实际 Worker 退役或析构 FIFO 完成混成一个指标 | `agents/libos-contract/browser-perf/`；同语义门禁看各端本身，不跨宿主比绝对时间 |

运行与读取规则见 [等待合同](wait-api-tdd.md)、[性能合同](performance-tdd.md) 和 [浏览器验证 fixture](../wasm-tombstone-followup/browser-fixture/README.md)。验证期开启 `verification-harness`，否则 Native 析构专用套件和 Browser 专用导出都不是测试证据。禁止用当前同步析构的速度填补缺少异步 owner 的结果，或以 mock 成功替代真实 Worker 证据。
