//! WASM `step` runtime override 的一次性设置与读取行为。

use std::sync::OnceLock;

/// 读取一次性开关；未设置时锁存缺省 `false`。
pub(super) fn step_runtime_override_enabled(override_lock: &OnceLock<bool>) -> bool {
    *override_lock.get_or_init(|| false)
}

/// 设置一次性开关；首次读取或设置后，后续设置均显式拒绝。
pub(super) fn set_step_runtime_override(
    override_lock: &OnceLock<bool>,
    enabled: bool,
) -> Result<(), &'static str> {
    override_lock
        .set(enabled)
        .map_err(|_| "step runtime override 已设置或已读取")
}

#[cfg(test)]
mod tests {
    use super::{set_step_runtime_override, step_runtime_override_enabled};
    use std::sync::OnceLock;

    #[test]
    fn 首次显式设置_true_后读取保持_true() {
        let override_lock = OnceLock::new();

        assert_eq!(
            set_step_runtime_override(&override_lock, true),
            Ok(()),
            "OVERRIDE_FIRST_TRUE_SET"
        );
        assert!(
            step_runtime_override_enabled(&override_lock),
            "OVERRIDE_TRUE_READ"
        );
        assert!(
            step_runtime_override_enabled(&override_lock),
            "OVERRIDE_TRUE_READ_STABLE"
        );
    }

    #[test]
    fn 首次显式设置_false_后读取保持_false() {
        let override_lock = OnceLock::new();

        assert_eq!(
            set_step_runtime_override(&override_lock, false),
            Ok(()),
            "OVERRIDE_FIRST_FALSE_SET"
        );
        assert!(
            !step_runtime_override_enabled(&override_lock),
            "OVERRIDE_FALSE_READ"
        );
        assert!(
            !step_runtime_override_enabled(&override_lock),
            "OVERRIDE_FALSE_READ_STABLE"
        );
    }

    #[test]
    fn 首次设置_true_后重复设置_true_或_false_均拒绝且保持_true() {
        let override_lock = OnceLock::new();

        assert_eq!(
            set_step_runtime_override(&override_lock, true),
            Ok(()),
            "OVERRIDE_REPEAT_INITIAL_TRUE_SET"
        );
        assert_eq!(
            set_step_runtime_override(&override_lock, true),
            Err("step runtime override 已设置或已读取"),
            "OVERRIDE_REPEAT_TRUE_REJECTED"
        );
        assert_eq!(
            set_step_runtime_override(&override_lock, false),
            Err("step runtime override 已设置或已读取"),
            "OVERRIDE_REPEAT_FALSE_REJECTED"
        );
        assert!(
            step_runtime_override_enabled(&override_lock),
            "OVERRIDE_REPEAT_VALUE_PRESERVED"
        );
    }

    #[test]
    fn 首次读取缺省_false_后迟到设置_true_或_false_均拒绝且保持_false() {
        let override_lock = OnceLock::new();

        assert!(
            !step_runtime_override_enabled(&override_lock),
            "OVERRIDE_DEFAULT_FALSE"
        );
        assert!(
            set_step_runtime_override(&override_lock, true).is_err(),
            "OVERRIDE_LATE_TRUE_REJECTED"
        );
        assert!(
            set_step_runtime_override(&override_lock, false).is_err(),
            "OVERRIDE_LATE_FALSE_REJECTED"
        );
        assert!(
            !step_runtime_override_enabled(&override_lock),
            "OVERRIDE_LATE_VALUE_PRESERVED"
        );
    }
}
