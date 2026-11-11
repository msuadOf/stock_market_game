#[path = "trace.rs"]
mod trace;

#[test]
fn 字段摘要保留顺序长度及字节差异() {
    let fields = vec![b"abc".to_vec(), b"abd".to_vec(), b"abc".to_vec()];
    let summary = trace::summarize(&fields);
    assert_eq!(summary.len(), 3);
    assert_eq!(summary[0].0, 3);
    assert_ne!(summary[0].1, summary[1].1);
    assert_eq!(summary[0], summary[2]);
}

#[test]
fn 选取原始字段并拒绝越界或非_utf8() {
    assert_eq!(trace::sample(&[b"abc".to_vec()], 0).unwrap(), "abc");
    assert!(trace::sample(&[], 0).is_err());
    assert!(trace::sample(&[vec![0xff]], 0).is_err());
}
