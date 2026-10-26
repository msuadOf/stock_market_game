# V11 runner 独审发现修复与未闭边界

2026-10-27。接受 e67 独审 R1/R2/R3，历史提交与所有原件保留；本轮没有实际WASM构建/运行或中央生产接线。

## R1：来源必须有真实receipt链

旧门只比较三个caller提供的source SHA，无法证明产物来自该源。`48fd32aa` 先保存可执行旧门与负测：缺receipt仍被接受，真实 `Missing expected rejection`。

当前新增专属 `verifyNativeReceiptChain` 消费既有正式main字段：

`native build-plan/source-before + build-result + artifact-binding → 实际normal rlib → trace compile plan/result → 实际trace binary → trace run plan/result → 同JSON输入和实际trace输出`。

每个receipt与实际文件均核bytes/SHA；source head、实际build-plan引用的source manifest、编译argv中的normal rlib、运行argv中的binary/输入/输出逐段一致。来源门在WASM实例化之前执行，不再接受单纯caller字符串声明。

除synthetic receipt字段测试外，还对现有真实6ac根receipt和e67 native trace完整执行此链：`runner-native-chain-6ac/` 记录exit0、0.767580秒。它验证旧6ac来源链，不能冒新staging或正确mixed市场行为。

**R1仍部分未闭**：W1 owner明确当前没有已审成功WASM build receipt/schema。当前对缺失或未冻结的WASM receipt显式拒绝，绝不初始化；这只修安全门，不能冒充正式WASM来源正控。最终root提供fresh build真实 `build-plan/sourceManifest/artifact-binding` 及准确schema后，必须机械消费、补真实正控并独审，才能关闭完整R1。

## R2：检查和实例化必须是同一个文件

旧门丢弃 `verifyWebReleaseWasm` 返回的唯一game路径，可能验证目录内新game却实例化manifest中的old.wasm。当前新增窄 `assertCheckedGamePath`，要求唯一返回文件resolve后精确等于manifest wasm路径；后续new WebAssembly.Module读取该同一已验路径。

真实红包括“合规game+另一个old.wasm”、空列表、多列表，修后均拒绝，同文件正控保留。

## R3：独立公开事实与timeseries oracle

保留原snapshot/根数/retained断言，新增专属 `assertMixedPublicTrace`：

- Jan2两份真实TickBatch公开facts不得携closed PriceTick/Trade/AuctionTick/AuctionCompleted；每拍有open PriceTick正控。
- 恰好一个DayBoundary，终K键严格只有open。
- 每拍公开timeseries的active_daily_candles/continuous_points/auction_points/closed_daily_candles不得含closed；每拍open continuous正控、末拍open终K正控。
- markets中的closed上一估值继续合法保留，不把估值读面误删成交易屏蔽。

不加载WASM的伪trace只污染公开fact或timeseries就必须被拒绝；不以native/wasm两边可能共同犯错的deepEqual替代此oracle。此测试不扩大真实成交或多核产品范围。

## 真实红绿与运行边界

| 证据目录 | 实际结果 |
|---|---|
| runner-r1r2-red | 旧门4case：3断言红、1正控绿；0.321078秒 |
| runner-r3-red | 旧snapshot oracle7case：6断言红、1正控绿；0.308344秒 |
| runner-r3-timeseries-red | fact修后、timeseries修前13case：6断言红、7绿；0.376636秒 |
| runner-r1r2r3-final-green | 最终21/21绿；0.190902秒 |
| runner-r1r2r3-green-parallel8 | 只修正并发参数后21/21绿；两file×每suite4，最大8case并发；0.188760秒 |

各目录保留执行前真实outer argv、source SHA/snapshot、实际stdout/stderr/result。Node case与整命令均10000ms，监督器为已审main v3绝对路径。前轮CLI写8不等于总case并发8：两个suite各8时实际可达16；最终明确收为两file×每suite4、最大8case，不改断言，前轮原件与错误预算保留。所有临时synthetic receipt文件均在独占根 `.tmp/mixed-calendar-c2c/` 下，case结束真实清理；它们只验证字段/hash关联，不冒真实Rust/WASM产物。

最终runner source SHA `098812e520227618318ed6c64ee2ca7e4b91060fcd0d5b5bc1c026ec1e4e033d`；独立oracle SHA `4378ba1a4dc9f91ec1433198bf36d09933edd4dc50c4bc0536ca8a26956f7713`；receipt门 SHA `7afc72e7d6edc5ec1c595aa164cf89f5a2937da982bdb186a7bbdc390c2f26a1`。本批待未实施者复核；不宣称R1完整来源门或实际WASM验收完成。
