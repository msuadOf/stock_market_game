# 保存测试两处类型收窄独审

结论：`SOURCE_PASS_WASM_SAVE_TEST_TYPES_NARROW_FIX`，无 finding。最终完整 patch SHA-256 `5ab14b6429516fa3794c88c275b415b0a6665335a7273321e3848c013dc0e483`，`worker-host.test.ts` SHA `7426bd270a80d041edcc8f46e69c9266921bc9832b8e1dbdb73d3ff415f95875`。实质准票已先直送唯一 writer 和父，不等本记录。

相对前次已准测试只有两处改动：先取 host.civilDate 并 assert.ok 确认 capability 后调用；先断言 sameRequest.requestId 为 number，再以局部别名构造对应 saved 和下一 civilDate 回复。运行成功/失败、发送数量、generation、FIFO、资源回收断言全部保留，没有 any、类型擦除或新增非空断言。其余六份 source SHA 逐一保持，git diff --check 通过。未重跑前次已过 51 项或主体矩阵。

作者 strict-recheck.log 明确完整 strict 两次由 10 秒进程树 deadline 终止且无 diagnostics，不能据此宣称通过；作者所报单 test strict exit0 在该目录只有叙述记录，审查者不冒称独立取得原始 argv/stdout。

唯一 writer 随后亲自分成三份必要 strict，沿相同 compiler flags、Node24、每命令 10000ms deadline 并行执行并报告真实 WAIT0：App session35300/等待工具 d53d71；host test session18526/368882；scope+capture tests session67720/239571，均自然 exit0、无 diagnostics。本记录区分 writer 的工具事实与本人的源码窄复核，不伪造重新实跑结果。

本次只收窄测试类型，不改变 A 股领域语义、产品逻辑、存档或金额单位。旧完整 SOURCE 报告与失败原件保留，本票允许唯一 writer 一次净接；Browser actual 仍须另验。
