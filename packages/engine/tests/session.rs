//! engine session 模块集成测试（TDD 红绿循环）。
use engine::session::SplitMix64;
use engine::strategy::Rng;

#[test]
fn splitmix64_is_deterministic() {
    let mut a = SplitMix64::new(42);
    let mut b = SplitMix64::new(42);
    for _ in 0..10 {
        assert_eq!(a.next_u64(), b.next_u64());
    }
    assert_ne!(SplitMix64::new(7).next_u64(), SplitMix64::new(42).next_u64());
}

#[test]
fn splitmix64_next_f64_in_unit_range() {
    let mut r = SplitMix64::new(123);
    for _ in 0..100 {
        let x = r.next_f64();
        assert!((0.0..1.0).contains(&x), "next_f64 out of [0,1): {x}");
    }
}

#[test]
fn splitmix64_next_range_u32_in_range() {
    let mut r = SplitMix64::new(999);
    for _ in 0..100 {
        let v = r.next_range_u32(10, 20);
        assert!((10..20).contains(&v), "out of [10,20): {v}");
    }
    assert_eq!(r.next_range_u32(20, 20), 20); // lo>=hi → lo
}
