# 根工作区历史证据清单

采集日期：2026-11-06；来源为根工作区未跟踪目录，基准 main 为 `5b258412229b12c36ca322d17622c4c24b746530`。本目录是历史材料，不是当前验收或交易规则依据。历史审查的批准仅针对当时冻结的补丁。

静态核对：main 的年末用例从会话目录取 CompanyId；WS fixture 未带三项旧残留；Worker 使用 classified code/message；runtime 显式识别 IssuerRepurchase。代码已演进，静态相似不证明历史补丁逐字应用或现行行为通过。未在本次运行历史测试，原始 binary、依赖、源码清单和日志未完整重新绑定；历史失败、未运行项目及限时缺口以各报告为准。回购台账所述官方核验日期为历史日期，本次未重新核验现行规则；以正式文档为准。

归档保留报告、独立复核和小型补丁；Worker 补丁以 Base64 封装以保留原始空白，解码后 SHA-256 与来源一致；源内相对链接改为历史路径文本，本机绝对路径以 `<源工作区>` 替换。未归档的原件保留在源工作区，尤其大体积构建采样、清单、日志及可能暴露宿主路径的执行绑定；原始日志未逐一人工审查，不应将其视为已脱敏。SHA-256 可用于识别来源和归档版本，并不证明历史测试真实性。

## 归档文件

| 来源（agents 下） | 来源 SHA-256 | 归档路径 | 归档 SHA-256 |
|---|---|---|---|
| `year-close-authority/diagnosis.md` | `22abcea48e011beede7598e363220d797efc0b786bfb23fb621ce177b55679c2` | `year-close-authority/diagnosis.md` | `22abcea48e011beede7598e363220d797efc0b786bfb23fb621ce177b55679c2` |
| `year-close-authority/sol-review.md` | `268b9215c7e2f1b8d5ecd8b83901ee53c12275a1c4424296569e75cb8aec836c` | `year-close-authority/sol-review.md` | `53e25c158839d1a9eb8f0f1b5973bc3f9c451971aa8d3f972cbe659f2fa0efbb` |
| `web-worker-error-fidelity/report.md` | `dbdf47fb8e1025fd1d4ecaa892b52d074fb95bf964208762f1736bd853e7656d` | `web-worker-error-fidelity/report.md` | `dbdf47fb8e1025fd1d4ecaa892b52d074fb95bf964208762f1736bd853e7656d` |
| `web-worker-error-fidelity/sol-review.md` | `29cb84cff4e600801b9e30fddfa11e33e4b5df1b0aeb3987441331366d6741c7` | `web-worker-error-fidelity/sol-review.md` | `0ad3e6e3b56f08fd240a913fd17e6f20a801c3c5682bce1d73d973de12238c09` |
| `web-worker-error-fidelity/change.patch` | `d259965bf5e14fa70dcbd446601ce035ee21d94c05b9ac75c34aca45a8cfd3a5` | `web-worker-error-fidelity/change.patch.b64` | `913c6523ee752402947dc47c9b39daa536974e5cbb4c769245847238e40d8649` |
| `ws-fixture-contract/report.md` | `1394a43876debe300db2e97b152071571375a1abb8e570235eccc165eb40d1ea` | `ws-fixture-contract/report.md` | `1394a43876debe300db2e97b152071571375a1abb8e570235eccc165eb40d1ea` |
| `ws-fixture-contract/sol-review.md` | `c065654b5011dc18fe89a10fc2275acc49691237de8be5046f675c8238967fe5` | `ws-fixture-contract/sol-review.md` | `0028c7312204992175b4a0d67f2fae234a9dbae43e61481454cf0bacd551b6b7` |
| `ws-fixture-contract/ws-fixture.patch` | `90509836871a83617dda6a19381c5b9a4fdeeb0339b9bb3b8d2e609764a76fa3` | `ws-fixture-contract/ws-fixture.patch` | `90509836871a83617dda6a19381c5b9a4fdeeb0339b9bb3b8d2e609764a76fa3` |
| `treasury-autonomy/ledger.md` | `a2d53ae1da6a47b4626b618d596ae58c1cf991b1fba32f778da177079e2f115c` | `treasury-autonomy/ledger.md` | `7eb255e3ee49808f47cedd0418dff3f60f6c5f945b9c496cbab97a66c2fd05f5` |

## 来源完整文件清单

| 路径（agents 下） | 字节 | SHA-256 |
|---|---:|---|
| `year-close-authority/build-and-bind.py` | 3912 | `f70e8ff9f72be2506d09c50abe4828ae01489104ece8d0b3beb306e72b953219` |
| `year-close-authority/diagnosis.md` | 7423 | `22abcea48e011beede7598e363220d797efc0b786bfb23fb621ce177b55679c2` |
| `year-close-authority/run-and-bind.py` | 3209 | `c6a6839fcbcad3e41677837d5f3d05f95633d1f7723dea4bc8ebe440bbae6984` |
| `year-close-authority/sol-review-binding.json` | 817 | `84f2e71d81f65bc65b7f231c2a4ad7607ab8eb0a3d214767cd548f9f0d1448d7` |
| `year-close-authority/sol-review.md` | 4708 | `268b9215c7e2f1b8d5ecd8b83901ee53c12275a1c4424296569e75cb8aec836c` |
| `web-worker-error-fidelity/change.patch` | 5823 | `d259965bf5e14fa70dcbd446601ce035ee21d94c05b9ac75c34aca45a8cfd3a5` |
| `web-worker-error-fidelity/freeze.json` | 1365 | `2e91f51da3f0be1281e6be81b60761f14360d37a70c7e23a786e730d85da2d25` |
| `web-worker-error-fidelity/green.json` | 5509 | `f2ed66253a47f5398eef5d9d38eb41b1fbc759b8fa0a279e4802479f6aaf1566` |
| `web-worker-error-fidelity/green.stderr` | 0 | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `web-worker-error-fidelity/green.stdout` | 3691 | `83acc80f1d35635437198ba7ab85d4dc545019aa4c325c48493414c4354d94b4` |
| `web-worker-error-fidelity/lint.json` | 270 | `2b274ebdc808f01bcdb6c31112473fb8fc88a2f96ea1e23112a888f0df7f049a` |
| `web-worker-error-fidelity/lint.stderr` | 0 | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `web-worker-error-fidelity/lint.stdout` | 0 | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `web-worker-error-fidelity/red-clean.json` | 5509 | `5362dd6b396ce12debdbd2b876427926a09d3de2c01aa66edaba7d6eebdb2dc9` |
| `web-worker-error-fidelity/red-clean.stderr` | 0 | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `web-worker-error-fidelity/red-clean.stdout` | 8796 | `34b070f3cee8715774dfb10f72ee34ba6eef05391ae4b9fd1641f457d49ee68d` |
| `web-worker-error-fidelity/red.json` | 5508 | `d13e716bbea06b23342d212ece8e18e029536cddd45228543f78218554433a19` |
| `web-worker-error-fidelity/red.stderr` | 0 | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `web-worker-error-fidelity/red.stdout` | 8688 | `a54b19bd5bfa55c5ef42945cbfbd1b506369f846ee73e6c46d88cd5f6bbd892e` |
| `web-worker-error-fidelity/report.md` | 3390 | `dbdf47fb8e1025fd1d4ecaa892b52d074fb95bf964208762f1736bd853e7656d` |
| `web-worker-error-fidelity/run-short.py` | 2336 | `1fe227eca33945a34ee632917deca3ddc7a75d32fa6b72cee9d6cce88c769927` |
| `web-worker-error-fidelity/sol-review-binding.json` | 1528 | `d8493ec5faa9222d90a00ef089b7019dd083a8997e9ef72c327aa176382963a6` |
| `web-worker-error-fidelity/sol-review.md` | 2552 | `29cb84cff4e600801b9e30fddfa11e33e4b5df1b0aeb3987441331366d6741c7` |
| `ws-fixture-contract/additional-workspace-source.json` | 2466 | `0348061510e68e35bc6365bd729d59b5a668c049f8805d5e9762e8eca16aedc8` |
| `ws-fixture-contract/build-cpu.jsonl` | 3336752 | `521bb534968c4fa0a296298ffaa6afd307c6ce7081c66eee7a73d3567c054532` |
| `ws-fixture-contract/build-result.json` | 989 | `8ae51ffce349ffa5a3f826f510a1ce8fadf8a986ad851cdc6d3df60d7b273caf` |
| `ws-fixture-contract/build-source-after.json` | 291133 | `bc86b2f742ee98cadabcfd6a43fe597636ec69c5669f6e579c9da082367f7407` |
| `ws-fixture-contract/build-source-before.json` | 291133 | `bc86b2f742ee98cadabcfd6a43fe597636ec69c5669f6e579c9da082367f7407` |
| `ws-fixture-contract/build-start.json` | 583 | `bb3bc37fc5aed9440f498f2bc0b71da23ec2948e0751b704d4b73d5550eb3ccc` |
| `ws-fixture-contract/build-terminal.json` | 927 | `e229cec3798e6cef50f475e9958b4029ee32a63469b52527fda154e2ab668566` |
| `ws-fixture-contract/build.py` | 4263 | `65cb1ebd63a1e7b2a04ddaaa039260db7623df7cac47ebb89e34aab7d8026bb2` |
| `ws-fixture-contract/build.stderr` | 273919 | `42775b58f0cb736527652eb743ed687923c2149ae93d976549be831c6e40b10c` |
| `ws-fixture-contract/build.stdout` | 230666 | `10fecbe9ed86d6bb1007572230c959ad90bfedbad50083d44f2157930c2e77a2` |
| `ws-fixture-contract/delivery-binding.json` | 1500 | `e017bf75deabe0fbb3805868c473fe50062d7392b9301e32da9554b702561f55` |
| `ws-fixture-contract/original-binding.json` | 857 | `177844451ba41e6adf9d611ff118770d651c4c39b75a39ee6f0251e917aebd83` |
| `ws-fixture-contract/original-red-result.json` | 1106 | `9e08bc8597b7cd67d8aef4d4ee72661c1b3fc622dac16ab13a14e514da81681b` |
| `ws-fixture-contract/original-red.stderr` | 32 | `5a69ce3158edc26e01b821801628e86fa98275bc800b90bfe8c26719957cafe7` |
| `ws-fixture-contract/original-red.stdout` | 677 | `5b2e7a259e10e4ab14a36c2ff8c50c538f5e08a7b9abc1a51c2b59ee83cb0449` |
| `ws-fixture-contract/report.md` | 4544 | `1394a43876debe300db2e97b152071571375a1abb8e570235eccc165eb40d1ea` |
| `ws-fixture-contract/run-shorts.mjs` | 5644 | `2322defa502a6973179c410a5ca88debbc7c6e2db7cf8b6e67286de2adff3de2` |
| `ws-fixture-contract/short-result.json` | 9577 | `8a0dbc8423caf70a82917b20898bf659f108fb46efb2a1e0a5fd1a4dc1c672c4` |
| `ws-fixture-contract/short-start.json` | 1117 | `b47f2d0d5562999762742d1f39d33838bb960e9024eac2f377518741a229aa2f` |
| `ws-fixture-contract/sol-review-binding.json` | 3169 | `ad67cc0daa47a18fb569715f8d444aab611aabe6059f93c87f3007121cd58333` |
| `ws-fixture-contract/sol-review.md` | 3848 | `c065654b5011dc18fe89a10fc2275acc49691237de8be5046f675c8238967fe5` |
| `ws-fixture-contract/source-baseline-full8.json` | 305636 | `267c5816c915ca46ca6278d473908acb7b05ce8ce12ab904a6b2e5bfdeed4484` |
| `ws-fixture-contract/ws-fixture.patch` | 666 | `90509836871a83617dda6a19381c5b9a4fdeeb0339b9bb3b8d2e609764a76fa3` |
| `ws-fixture-contract/ws-listings.json` | 1460 | `64ee4590f7940e8b83266ca087702b5e58a664d1a70f47d23be5bd7ff58973a3` |
| `ws-fixture-contract/ws-original.rs` | 33552 | `caf057711e10583f29c0a59d52a7002409ff1192af3d554696840ed07bf3690c` |
| `treasury-autonomy/ledger.md` | 6661 | `a2d53ae1da6a47b4626b618d596ae58c1cf991b1fba32f778da177079e2f115c` |
