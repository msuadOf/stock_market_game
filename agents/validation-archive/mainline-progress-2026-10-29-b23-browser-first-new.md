# 主线阶段验收：B23 与真实 Browser 首次新局

截至 2026-10-29 23:10（Asia/Shanghai）。仅存小文本摘要；不上传 binary、target 或大日志。原失败保留。

## 已核事实

- 唯一开发主线 e460aefa3321d47edd5e6c4ec0555c6825b9f87c；包含 H1/SaveRoot、B、性能入口与 N/2N 诊断源码。源码已接收不等于整树通过。
- #122 B 净提交 d5d4e06b130f29c359e6943c2fd5cd00a916e5e7，开发主线等价 758b2f02。真实 session40186、WAIT0、chunk1735a8，113.173秒，23/23；公开flush、目标日存档、直接step及重复成员移交通过。原编译失败和15绿8失败原件仍保留，front SHA分别为e5fac08343d6c15078b090d67d553c0c66bc339ccf133c3036d073db94b057bb、3a264411c7fc9f06fff494ccde2cbebcc8b8d0c690858e490236acf182e93e36；root另亲读r2 execution.json核23项15绿8失败。
- B23清场front SHA256：6420e9b4d08ad76c2f6c5db3fa0919cf0a734e408ae1e922d1da5c173aa12957。28source/4runner、fresh:false本源artifact、原源时间恢复及四项清场均独立核验。
- 首次新局真实 Browser：session28619，launch79f13f、WAIT0/96d8e3。普通document owner→production factory→World tick1/day0→三项查询→Rust正常shutdown。ownedWorkers=joinedWorkers=2，group/desk Final各1，childExit各到、双quiescence、4个Worker关闭。同一allocationId=1覆盖engine/ingress/child，finalBytes=75759616。
- Browser manifest与candidate逐字同，SHA256：2020ea45e8f3b36dd6b2889a181e82f95beb335c883948559ee10f23b3666adc；真实harness SHA256：8752c4dbee59fc7cdbd0ff2645e2a8e5b318ba6daddcb721ca0b7452e444f75a。
- Browser清场front SHA256：8a5d09d18bf38169a5b09299aa5506efeeb45021a4f19d3ea46a2858db1f1b46。四项剩余清单为空、scratch不存在、ECHILD为真；未实施Sol已复核有限actual。

## 适用范围与未完成项

Browser产品源固定132638eb027d48acbc12a63cdb9ff731ec2f1c8b；复用fresh02真实release构建的确切pkg/dist，04仅必要新测试harness，compiledInThisBatch=false。HTTP采用测试preview no-store，实际200/bytes/SHA核验；不声称覆盖自然304、普通App UI、完整日结同X恢复、reload/F5或20局。

#122 C checkpoint防绕过、真实更正prepare不可伪造来源、宿主失败交付链及Worker关闭窗口仍在修复/验证。完整Web曾打印1515pass/147fail（含failfast文件），相关修复尚待最新整树复验。

性能入口与N/2N源码已独审；Node fake通过不等于Rust或真实性能通过。c542小样本编译正在执行，未有正式ABBA、N/2N或1000倍实时速度结论。最终全仓回归/整树终审/本地main合并尚未闭合。

本地main仍bc8a2add；远端main仍f031407e9843f5824e93cedb730480151efc0803，未push产品。本摘要仅上传独立evidence分支。
