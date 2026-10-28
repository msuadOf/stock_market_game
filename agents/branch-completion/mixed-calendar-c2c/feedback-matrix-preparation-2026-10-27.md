# V4 旧矩阵准备冻结与执行接力

2026-10-27。该准备先于首片两case完成，保留原件，不因收敛首片而删除或覆盖。

- 源文件 `packages/engine/tests/mixed_calendar_feedback.rs` SHA `073a317632b01988b6bdc843be5ce5b185775c68b9c965f1d9ccb990e76ef2f4`。
- 复用原helper SHA `d1120a2ad62e3d0c1ca15ec1126ec44cf2050b48c4f84cb07e4dafbb5f4b2eb3`，未修改。
- 实际仅编译过：1.004868秒、wrapper exit0；6ac旧normal rlib，不冒新stage或本树生产重建。完整计划/stdout/stderr/result在 `feedback-red/`；一个helper未用常量warning原样保留。
- binary 根 `.tmp/mixed-calendar-c2c/feedback-red-6ac`，49457480 bytes，SHA `6a7688284d9cf2ab869c14bf80d1464da8bc262fb4abdebd1fded7b40eb15ae6`。
- 17个已写case包含Limit/Market/Cancel×phase、公开资源/serial/plan投影、资金不足优先级、真实受理ordinal和open正控；这些矩阵case**本writer没有运行**，不报告已得红或绿。源码中的公开plan投影断言不能替代组私有PlanId游标门，Cancel使用未知order验证closed优先级，不冒已有活动closed委托场景。
- 协调者已指定该source/helper/binary只读冻结，由fresh Luna独立核SHA后执行，证据写根 `feedback-matrix-run-2026-10-27/`。本提交只封存writer已有准备，不修改、重编或运行它。

首片独立两case的真实open绿/closed反馈红见提交 `978dedf8` 与 `feedback-first-red/`；两个测试文件分别归档，不能把首片结果冒全矩阵运行。
