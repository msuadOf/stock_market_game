# G55：公开策略输入校验

MomentumStrategy::new拒绝非有限trend_threshold，StrategyParams::validate和Factory沿用同一入口。Factory对零ticks_per_day返回InvalidParam而非debug panic/release退化；机构原始margin在个体风格钳位之前按[0,1)核实，不用min掩盖非法输入，也不消耗RNG。

3项新增短case在原码全部失败，修复后全部通过（0.00s）；另独立运行factory_builds_each_kind合法实例对照，1项通过（0.00s）。构建`--no-run -j 16`；执行`RAYON_NUM_THREADS=8 timeout 10s <strategy-binary> <filter> --test-threads=8`。没有回归或长验收。

非作者restore_batch_review独立审查完整产品/测试diff，三门禁通过，没有阻断发现。按其建议同步参数/构造器文档为有限且非负；最终台账再次复核通过。改动只拒绝非法游戏参数，不改变合法个体差异、A股制度或撮合。
