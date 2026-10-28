# V11 native/实际 WASM 对拍准备

2026-10-27。当前没有最终 staging WASM 包，**Node 对拍未执行**；不把旧包、Node原生runtime budget1或浏览器bootstrap协议测试冒充实际wasm32市场路径。

## 已实际执行的 native 输入链

`trace-native-public-fixture.rs` 每次读取独占sealed JSON的实际字节，经生产 `decode_save_slot(..., SaveDecodeLimits::default())` strict decode，再 `ProtocolSession::restore`，没有复用exporter内存struct。

在6ac旧normal库上的实际轻编译与运行都通过，只证明记录器与公共输入链可运行：

- source SHA `3d446109ba7a36f2da3fcbf43d0a19cf7fc215cb455313f8d8e92c422432c3ac`。
- compile1.026570秒、exit0；binary64664576 bytes，SHA `70a9345c7f740e622176cfae38a69411f299e950fe99467c843dae7f03d74ff1`。
- run0.856209秒、exit0；读取实际输入SHA `63d03f04a821ba9a84d8e35d20f2c032415afd95d0dca5d085ad37f00f3fb1ae`。
- trace位于根 `.tmp/mixed-calendar-c2c/native-public-trace-6ac.json`，1571950 bytes，SHA `4b478311d4edbb0bea5c20fc8d91e8a34400618003feea732063f9dde78d7e59`。
- trace真实记录两拍→civil→复市两拍→civil的6份公共update与snapshot/minute/retained。它仍忠实携带6ac旧行为（首日两股均361根），不能当正确oracle或最终生产绿。

计划/实际stdout/stderr/result完整保存于 `native-public-trace/`；新build/run均使用main树v3绝对监督路径，实际outer argv执行前已记入plan。没有重跑或改写旧57a/82ce/e296证据。

## 已准备、未执行的实际 WASM runner

`compare-native-wasm-public-fixture.mjs` 使用现有exports，不加依赖/API：

- 读取最终同source manifest的native trace、真实wasm、同批glue和同一个public JSON；用现有 `hashArtifactFiles` 8线程校验完整source清单与产物，运行后再核相同hash。
- 调用现有 `verifyWebReleaseWasm` 核真实wasm/import/原子码位，不加载旧W0 bytes。
- Node `registerHooks` 只把裸 `env` 精确映射到同一真实 `engine-clock-env.ts` URL；复用真实facade binding及 `normalizeSerdeMaps`，不复制shim，不伪造crossOriginIsolated/DedicatedWorker。
- 已由W1 owner明确的现有合同：`Memory({initial:1024,maximum:32768,shared:true})`；先 `bindFacadeThreadEnvironment`，再 `glue.initSync({module,memory})`；核raw.memory同一实例和真实destroy导出。
- 当前准备显式fixture arena容量16，仅是验证输入，不冒NPC组数/硬件预算；`facade_slot_init(16)`→真实layout→install→arena读面。任何facade控制消息直接失败，不声称browser supervisor或ACK已跑。
- 调用真实restore_json，按与native相同次序跑6次step、分钟/retained查询，保存输出后核closed独立oracle，并逐字段比较完整update/市场/账户/分钟/终K/retained。

W1 owner明确当前未来 `facade_slot_init_for_setup` 尚未交付；本runner没有调用它。最终实际staging如果变更init合同，须机械适配其已审真实exports并重新冻结，不能凭本文抢先接不存在API。

## 最终 manifest 与执行门

实际包准备好后才生成manifest，不能先填写假hash。runner要求：

- `kind=c2c-native-wasm-public-fixture`、真实 `sourceHead/sourceRoot`；`engineRuntime=wasm-serial`。
- `sourceManifest`携真实path/bytes/SHA且内容为head/files清单；native与wasm两次构建真实绑定同一manifest SHA。
- `wasm/glue/fixture/nativeTrace/sourceFiles`各携实际绝对path/bytes/SHA；sourceFiles至少覆盖实际env、facade、normalizer、runtime门与桥源码，完整source清单逐项重核。
- `fixtureCapacity=16`与实际outer argv；outer的Node和监督器均为已授权绝对路径：根 `.tmp/node24-env/node`、`.worktree/main-merge-2026-10-25/scripts/run-with-deadline.mjs`。

普通代表性对拍使用case与整命令10000ms外部deadline。真实WASM/native构建窗口由root统一排、内部显式多核、每阶段≤300000ms；本轮没有请求或执行重构建。Node串行fixture遵循wasm宿主固定串行语义，hash阶段仍8线程；不通过并发同一realm重绑env来伪造多核。

本准备仅覆盖零NPC/零成交代表性V11路径。真实成交、T+1、plan与typed feedback仍须最终同分支其他fixture闭合，不能凭本对拍替代；私有分钟首键继续由已写engine测试直接验。
