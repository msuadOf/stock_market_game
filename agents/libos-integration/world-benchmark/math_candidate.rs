/// 使用隔离的纯 Rust 数学实现评估已有输入，并经过当前诊断工程的 JSON 往返。
pub fn evaluate(argument: f64) -> Result<(u64, u64), serde_json::Error> {
    let probability = -libm::expm1(argument);
    let encoded = serde_json::to_string(&probability)?;
    let decoded: f64 = serde_json::from_str(&encoded)?;
    Ok((probability.to_bits(), decoded.to_bits()))
}

#[cfg(test)]
mod tests {
    use super::evaluate;

    #[test]
    fn 候选在已知高精度参考值附近并完成_json_往返() {
        for (argument, expected) in [(-0.04_f64, 0x3fa4_1368_18ff_472b_u64), (-0.1_f64 / 100_000_000.0, 0x3e11_2e0b_e801_f1d9)] {
            let (raw, canonical) = evaluate(argument).unwrap();
            assert!(raw.abs_diff(expected) <= 1, "候选值偏离高精度参考：{raw:016x}");
            assert!(f64::from_bits(canonical).is_finite());
        }
    }
}
