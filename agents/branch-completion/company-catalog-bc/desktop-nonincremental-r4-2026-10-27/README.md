# Desktop 日终存档前置夹具修复 r4

r3真实构建成功11.410秒，四必要exact三过一败。失败在protocol_tests.rs285 game.save自身抛fault3015，尚未进入restore或故障注入；这是未完成首次自然日日结的合法拒绝，不能据此宣称catalog fail-stop已被测试。密封r3binary、该首失败stdout/stderr均原样保留。

唯一增量是在原2030-01-01默认自然日调用真实end_civil_day_update后才save（同文件baseline_and_restore和restore_rebinds既有合法模式）；不绕到GameSession私有save，不放宽ProtocolSession日终存档规则。原真实actor.restore、Catalog错误、generation2、timeline保留、cmd关闭与新ingress拒绝全部断言不改。生产源码沿已独审r3两map_err不再变化。

source.sha256重新冻结15项；INC0/原target/no-default-features/catalog lib-no-run/j8/300s/60s/20GiB保持，等待独立静审与root正式唯一槽。原三次失败／运行绿不覆盖，r4尚未运行。
