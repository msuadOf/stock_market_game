# 第二次四包批次的准备范围

本目录是首次编译失败后的独立重试准备，尚未执行。唯一产品 delta 见 `../green-candidate-v3/compile-fix.diff`：Tauri `lookup_handles` 的 `SendCommandError` 显式转为 `String`，保留错误原因。

沿用 green01 已审的十八个 exact、八进程并发、10000ms 单命令进程树截止，以及共享 297／299／300 秒绝对截止。`run-ram-batch.py` 只将本批 RAM owner 路径从 green01 改为 green02；launcher、绝对监督器、cases 原样复制。plan 中相应输出路径、源码及 runner hash 随之更新。没有修改 profile、features 策略、通用 helper 或 collector。

slot 及 protectedPrior 均 pending，启动检查明确拒绝运行；等待 E2 实际退出收口与 root 再次分配后，才可绑定真实前槽及窄复核。原 green01 失败记录不覆盖、不重跑。

未实施 reviewer `/root/sol_company_queries_28/query_red_runner_review` 实际窄核通过，结论为 `READY_PENDING_SLOT`：runner 经唯一 RAM owner 01→02 替换后与 green01 逐字相等，launcher／supervisor／cases 字节相同；两份 manifest 的 1001 sources／8 runners／18 cases 一致、无删项，现 hash 全部匹配，仅 Tauri `lib.rs` 的源码 hash 更新。命令保持 j8／INC0，plan SHA `0bb9550c952d92460e0d92eaad61d11688cd77f95ecd9c9f15c650b7275e2909`、freeze SHA `1a9a725c4c9b990bd5e83d3b4ef096c1649b161a5f7141a2f33b0c220c234293` 与 binding 相符。未运行 Cargo 或 runner，不授权启动。
