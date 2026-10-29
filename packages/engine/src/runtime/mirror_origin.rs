//! 公开运行镜像来源，用于标识同一可见镜像的来源截点。

/// 日终镜像处于的发布阶段。
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
#[ts(rename_all = "snake_case")]
#[ts(export)]
pub enum RuntimeMirrorDayEndStage {
    Tick,
    DayEndCommitted,
    RetainedPublished,
}

/// 运行镜像的权威版本与来源进度。
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct RuntimeMirrorOrigin {
    #[serde(with = "crate::orderbook::js_safe_u64")]
    #[ts(type = "number")]
    pub authority_epoch: u64,
    #[serde(with = "crate::orderbook::js_safe_u64")]
    #[ts(type = "number")]
    pub install_sequence: u64,
    #[serde(with = "crate::orderbook::js_safe_u64")]
    #[ts(type = "number")]
    pub source_tick: u64,
    #[serde(with = "crate::orderbook::js_safe_u64")]
    #[ts(type = "number")]
    pub source_generation: u64,
    pub day_end_stage: RuntimeMirrorDayEndStage,
    #[serde(with = "crate::orderbook::js_safe_u64")]
    #[ts(type = "number")]
    pub non_tick_revision: u64,
    pub mirror_phase: crate::TradingPhase,
}

impl RuntimeMirrorOrigin {
    /// 校验所有 `u64` 来源游标可由 JavaScript `number` 精确表示。
    pub fn validate(&self) -> Result<(), String> {
        for (field, value) in [
            ("authority_epoch", self.authority_epoch),
            ("install_sequence", self.install_sequence),
            ("source_tick", self.source_tick),
            ("source_generation", self.source_generation),
            ("non_tick_revision", self.non_tick_revision),
        ] {
            if value > crate::orderbook::js_safe_u64::MAX {
                return Err(format!("字段 {field} 超出 JavaScript 安全整数范围"));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{RuntimeMirrorDayEndStage, RuntimeMirrorOrigin};

    fn origin() -> RuntimeMirrorOrigin {
        RuntimeMirrorOrigin {
            authority_epoch: 1,
            install_sequence: 2,
            source_tick: 3,
            source_generation: 4,
            day_end_stage: RuntimeMirrorDayEndStage::Tick,
            non_tick_revision: 5,
            mirror_phase: crate::TradingPhase::Continuous,
        }
    }

    #[test]
    fn serde_requires_every_origin_field() {
        let value = serde_json::to_value(origin()).expect("origin should serialize");

        for field in [
            "authority_epoch",
            "install_sequence",
            "source_tick",
            "source_generation",
            "day_end_stage",
            "non_tick_revision",
            "mirror_phase",
        ] {
            let mut missing_field = value.clone();
            missing_field
                .as_object_mut()
                .expect("origin should be an object")
                .remove(field);

            assert!(
                serde_json::from_value::<RuntimeMirrorOrigin>(missing_field).is_err(),
                "missing field {field} must be rejected"
            );
        }
    }

    #[test]
    fn serde_rejects_unknown_origin_fields() {
        let mut value = serde_json::to_value(origin()).expect("origin should serialize");
        value
            .as_object_mut()
            .expect("origin should be an object")
            .insert("unexpected".to_owned(), serde_json::json!(1));

        assert!(serde_json::from_value::<RuntimeMirrorOrigin>(value).is_err());
    }

    #[test]
    fn validate_rejects_each_integer_outside_javascript_safe_range() {
        let unsafe_value = crate::orderbook::js_safe_u64::MAX + 1;
        let mut cases = [origin(); 5];
        cases[0].authority_epoch = unsafe_value;
        cases[1].install_sequence = unsafe_value;
        cases[2].source_tick = unsafe_value;
        cases[3].source_generation = unsafe_value;
        cases[4].non_tick_revision = unsafe_value;
        let fields = [
            "authority_epoch",
            "install_sequence",
            "source_tick",
            "source_generation",
            "non_tick_revision",
        ];

        for (case, field) in cases.into_iter().zip(fields) {
            let error = case
                .validate()
                .expect_err("unsafe integer must be rejected");
            assert!(
                error.contains(field),
                "error must identify {field}: {error}"
            );
        }
    }
}
