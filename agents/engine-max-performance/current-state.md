# 当前状态与继续入口

## 已保存的本地工作

- 工作区 `.worktree/engine-max-performance`，原始基线 `a6001494`，分支 `fix/engine-max-performance`；不修改主工作区的未提交内容。
- `d29df69b`：AccountPage 资源成功校验缓存，真实TDD + Luna独立审查。
- `44ed1f07`：Worker MessageChannel即时task调度，14短测 + tsc + WASM threading + Luna独立审查。
- `3b52f1c2`：ledger不可变map共享/Weak identity审计缓存/unchanged rebase，红绿+费用/失败/原子性/hash/序列化短测+Luna两轮复核；CPU probe记录实际进程与线程占用。
- 详细事实见findings.md和各review.md。原始.json/.log因仓库ignore保留在当前工作区，不在commit内。

## 目标与当前实测

- 原始目标16核默认20,007NPC、5股max>=1000x，仍未达到。不能降低NPC数、虚报tick单位或跳过权威工作。
- 最初原生16worker3000tick约78x、真实Browser约22x。当前原生16worker约88x、4worker约113x；Browser约28x。短开盘和连续竞价窗口不能混用；长期/全交易日尚未验收。
- 最新CPU采样平均16worker250%CPU，即16核预算15.6%；主调用线程55%，worker7–24%。更多worker当前反而减慢，需要减少串行循环和细碎调度。
- 用户追加要求检查能否吃满分配CPU；cpu-probe.py记录processCPU、per-thread下界、CPU affinity和独立系统核占用。不得把其他进程或忙等算为有效引擎计算。

## 在途工作

- Root正在account_book.rs/failure.rs增加strategy presence页缓存；真实红测脚手架已写，构建session81868（用functions write_stdin核实；不要凭本文件当成live证据）。新增测strategy_presence_reuses_unchanged_pages_and_rechecks_mutation先跑红灯，再加OnceLock<bool>失效字段。
- 一次性Luna high subagent parallel_ledger_audit正在独立实施ledger_conservation.rs/ledger.rs必要全量行审计并行，不能和root同时改其文件。需收到产物后独立review、短测、实测，再提交。
- 未来重头方向：ledger/订单簿/输入投影每tick重复全量工作。目前SharedRows只是整map COW；一个修改仍可能复制整map。考虑有序账户组COW和按组审计cache，保持价格时间优先、全局守恒与失败原子性。
- Vite以提权本机loopback5187运行，session74103；先核实live，再决定复用。浏览器任务均由299秒TERM+1秒KILL外部监督；CPU probe和native一样受外部deadline；普通测试是9秒TERM+1秒KILL或Node10000ms监督+case10000ms。

## 构建/测试复用

- Native release target `.tmp/perf-target`，默认探针 `.tmp/perf-target/release/examples/max_speed_probe default-setup.json 3000 16`；probe用globalRayon池，host调用线程不占worker。
- lib测试 `.tmp/perf-tests/debug/deps/engine-6ae5d5d49d140673`，每次根据当前源码重新构建再运行；文件名相同不意味着源码相同。另有 `.tmp/ledger-optimized-test-binary`旧binary仅供历史证据，不作当前结果。
- `.tmp/ledger-baseline`是HEAD44ed1f07的独立archive，已重编复现旧fixture失败；该source不含最新优化，用于基线证明。
- WASM缓存target绝对路径 `.../apps/web-wasm/.tmp/perf-target`，nightly-2026-09-05，wasm-pack输出apps/web/wasm-pkg（ignored）；禁止相对CARGO_TARGET_DIR，不然Cargo/crate与wasm-pack根路径不一致。
- node和apps/web/node_modules为指向主workspace依赖的symlink；只stage具体本任务源码与文档，勿提交node_modules/generated binaries。
- perf采样受系统perf_event_paranoid=4禁止，未改系统设置；可以用phase timing和/proc CPU采样。
