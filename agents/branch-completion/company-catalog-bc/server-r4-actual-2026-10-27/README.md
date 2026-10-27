# Server r4 实际构建与必要 case

- root 正式交唯一 Cargo 槽后运行既有 `company-catalog-server-r4-2026-10-27/build-plan-v3.json`，SHA `fce235990f53e0eae6b21293110b95e12edad2955d61e44198de9311e52eaa53`；原 Server r3 Cargo101 失败原件未覆盖。
- session `37238` 已实际 wait，exit 0；外部 300000ms，内层 cleanup reserve 1000ms、60秒无进展停止、磁盘低于20GiB停止。实际 build 40.237秒、80份CPU样本，参数 `-j8`。末次可用24.194GiB，未触阈值；单crate阶段和链接器采样不被描述为满核。
- 原日志与收据在根 `agents/branch-completion/company-catalog-server-r4-2026-10-27/`，compiler-artifact `fresh:false`，实际目标 `server-1f89281b24100faf`。COW 密封于 `.tmp/company-catalog-bc/server-r4-sealed/server-tests`，SHA `6c77d7e2bcd0f920049d013c022e739a63cb9b6dfee189a28cb248101024b62b`；不将 ELF 加入提交。
- `cases.json` 和三份 stdout/stderr 为密封 binary 的实际运行。三个 exact 各1/1通过，分别验证 typed catalog leaf/3019、同Session baseline目录、故障后真实actor ingress关闭；三个进程并发，每个进程外部10000ms、`--test-threads=8`，最长0.224秒。
- `server-ready-r4/source.sha256` 13项构建前、运行后均逐项通过；未修改冻结生产源码。实际等待后 Cargo/rustc 树为空，槽已交root；没有自动接续 Desktop 构建。

这是 native Rust Server seams，不是远程真实浏览器交易旅程，也不证明 SOURCE/R4、完整三host或整体B/C完成。Server一行测试修复已有未实施者独立静核；完整B/C既有独审与其未完成边界继续有效。
