use engine::{session::protocol::ProtocolSession, SaveSlot};
use std::{env, error::Error, fs};

fn main() -> Result<(), Box<dyn Error>> {
    let path = env::args().nth(1).ok_or("缺少存档路径")?;
    let save: SaveSlot = serde_json::from_str(&fs::read_to_string(path)?)?;
    match ProtocolSession::restore(&save) {
        Ok(_) => Err("旧 producer 存档意外通过公共 ProtocolSession::restore".into()),
        Err(error) => {
            println!("旧 producer 存档被公共 ProtocolSession::restore 拒绝：{error}");
            Ok(())
        }
    }
}
