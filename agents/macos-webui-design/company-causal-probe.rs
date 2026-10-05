//! 只读审计：相同 seed/setup 的首份公开报告，不推进市场或修改账套。
use engine::session::{GameSession, SessionSetup};
use engine::company::PublicReportQuery;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).ok_or("缺少 setup 路径")?;
    let setup: SessionSetup = serde_json::from_str(&std::fs::read_to_string(path)?)?;
    let session = GameSession::new(setup, 42)?;
    let page = session.query_public_reports(&PublicReportQuery { company_id: "C-600101".into(), cursor: None, page_size: Some(1) })?;
    println!("{}", serde_json::to_string(&page)?);
    Ok(())
}
