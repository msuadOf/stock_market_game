# 2026-10-29 旧磁盘编译 cache 稳定余量清理

状态：精确19 ELF已通过独立审查并实际删除，command exit0；以下保留来源、限制和实际结果。root依据用户“及时清理不用文件、证据可小Markdown替raw”新授权，要求在restore已继续运行后增加至少8GB的/data1稳定余量；本批不触运行中的shared RAM target。

只涉及一个目录：`/data1/baiyifan/workplace/stock_market_game/.tmp/build-cache/full-regression/debug/deps`。该目录allocated 37,448,294,400B，含99个旧无扩展名test ELF共33,035,882,496B，其中nlink1可独立删的96件共32,222,965,760B；本批仅从这96件按allocated降序取达到8GiB的最小前缀 **19件/8,788,860,928B（达到8GiB的最小前缀）**。预检发现部分ELF为nlink2，全部排除并stat保护；只选nlink1独立inode。同目录余项stat冻结保护，全部rlib/rmeta/.so/.d/.fingerprint、源码、runner和小日志均保留；finish125整目录约2.54GB本批不动，不删除整个39GB cache。

| 精确ELF名称（均在上列目录） | allocated bytes |
|---|---:|
| `engine-ec537d051c5d4a67` | 588869632 |
| `stock_market_game_lib-0ef0880103169caf` | 553828352 |
| `api_contract-ac113fe2018da2ba` | 497930240 |
| `ws-dde321042f9ed124` | 495861760 |
| `server-a5d88607731fa10f` | 480288768 |
| `shared_market_rest-fff12d45ce2fa318` | 478793728 |
| `pause_security-c5d2c520b871fb5f` | 470966272 |
| `identities-f2fbae170f81386f` | 470433792 |
| `actor-3984bf2fb3c53cdb` | 470396928 |
| `healthz-65c482952f603812` | 469340160 |
| `deployment_options-6f593fe731093a51` | 468160512 |
| `protocol_updates-5cf6efb50fa9ae75` | 457351168 |
| `shared_market_restart-c1e29dc9a4c183e1` | 456732672 |
| `producer_guard_isolated-6abfc04b91732992` | 454504448 |
| `session-f11c58478eb89c71` | 403714048 |
| `save_contract-bacf95536b2ec239` | 395501568 |
| `publisher-27b3c51411880931` | 392720384 |
| `company_operations-e87e2a3e89092601` | 392581120 |
| `identity_fixture-756b936d2505b209` | 390885376 |

## 来源、原运行结果及限制

这是可重新构建的旧Cargo test ELF cache，mtime UTC区间 `2026-10-25T07:50:51.445532+00:00` 到 `2026-10-25T09:02:11.658811+00:00`。`.d`和Cargo fingerprint小元数据保留，支持识别crate与source文件路径。未取得这本批逐件可靠的source Git SHA、run状态或原binary SHA；不把当前HEAD、其他历史全绿报告或ELF存在冒充它们的实际来源/结果，也不为清cache重新大hash或重跑测试。当前维护前HEAD `af0fdeb6c952f294e3069f031c38f4d8ad0fd6b4`仅用于保护主checkout，不是旧cache源证明。

原release/全回归小日志和manifest仍在`agents/main-release-validation/`等既有目录，本批没有删除、迁移或改写它们。不能仅凭共用cache目录就把某次报告绑定到这里每个ELF。真正直接后继的restore/Calendar/E2/Host均使用 `/tmp/stock-market-game-validation-2026-10-27-1311109/target`，owner及小runner闭包确认后才准删；无法取得的历史SHA/运行结论如实不承诺。

删除日期及实际退出将追加；删除后不能再对旧raw字节复核/直接重跑原ELF，但仍可从保留源码重新构建，不承诺重建字节相等。没有新压缩、跨盘迁移或无限保raw。

## 执行与保护

精确stat清单见cache-stability-2026-10-29-plan.json；全部普通同UID、nlink1、ELF头核验，删除前再核stat及可读/proc exe/cwd/maps/fd无选件引用；受限PID如实记录。8worker并行独立unlink，共享外部300秒deadline，每条普通只读/sync命令10秒。删后sync再核/data1、同目录非选择项stat与HEAD/index不变。restore可能合法更新RAMtarget current，因此本批不把其合法变化当清理漂移，也不读写/复制/hash它的大件。产品源码、契约、规则、断言、金额/股份单位与A股语义均无改动。

## 实际执行与停止（2026-10-29）

未实施gpt-6.1-sol high批准修正后的nlink1精确选择后，8worker实际删除19件，sync exit0，outer chunkb19761亲收 **exit0**，wall约0.893秒。19原件已于2026-10-29删除；1665非选stat、3个hardlink ELF、finish125和HEAD/index均保留原样，sharedRAMtarget未读写/hash/copy。真实收件为cache-stability-2026-10-29-receipt.json。

原逻辑allocated为 **8,788,860,928B**；data1全盘实际available从 **24,174,862,336B** 到 **24,969,011,200B**，实际观测仅增 **794,148,864B**。**≥8GB物理净增未证实，原因未查明。** 不能把allocated删除量写成同等物理释放，也不强断言这个全盘差值由reflink/外部writer/延迟中的哪项造成。当前可用仍超过20GiB硬门约3.25GiB；root明确余量已足，停止此批，不继续清finish125、同名克隆cache或其它路径。

四批合计35个选择路径/44普通文件，原逻辑allocated **18,197,491,712B（约16.95GiB）**；此前三批与本批全盘df并非同一时点，不能据其简单累计出物理释放总量。未新增归档/备份，不再扩大盘点。
