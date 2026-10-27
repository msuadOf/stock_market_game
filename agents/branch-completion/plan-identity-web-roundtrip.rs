// 只读验证 Web 重序列化结果能被生产 Rust 深校验并恢复。
fn main() {
    let path = std::env::args().nth(1).expect("输入文件路径必填");
    let bytes = std::fs::read(path).unwrap();
    let save = engine::session::decode_save_slot(&bytes, &Default::default()).unwrap();
    engine::GameSession::restore(&save).unwrap();
    println!("Rust decode/restore通过；计划数{}", save.plans.plan_ids().count());
}
