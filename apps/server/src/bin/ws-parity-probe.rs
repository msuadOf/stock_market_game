use std::env;

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio_tungstenite::tungstenite::{handshake::client::generate_key, http::Request, Message};

fn required_arg(index: usize, name: &str) -> Result<String, String> {
    env::args()
        .nth(index)
        .ok_or_else(|| format!("missing required {name}"))
}

async fn next_json(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> Result<Value, String> {
    let text = ws
        .next()
        .await
        .ok_or("websocket closed")?
        .map_err(|error| error.to_string())?
        .into_text()
        .map_err(|error| error.to_string())?;
    serde_json::from_str(&text).map_err(|error| error.to_string())
}

async fn next_named(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    name: &str,
) -> Result<Value, String> {
    for _ in 0..8 {
        let value = tokio::time::timeout(std::time::Duration::from_secs(2), next_json(ws))
            .await
            .map_err(|_| format!("timed out waiting for {name}"))??;
        if value.get(name).is_some() {
            return Ok(value);
        }
    }
    Err(format!("did not receive {name}"))
}

#[tokio::main]
async fn main() -> Result<(), String> {
    let base_url = required_arg(1, "base URL")?;
    let session_id = required_arg(2, "session ID")?;
    let mut input = BufReader::new(tokio::io::stdin());
    let mut token = String::new();
    input
        .read_line(&mut token)
        .await
        .map_err(|error| error.to_string())?;
    let token = token.trim();
    if token.is_empty() {
        return Err("missing session token on stdin".to_owned());
    }
    let ws_url = base_url.replacen("http://", "ws://", 1);
    let request = Request::builder()
        .method("GET")
        .uri(format!("{ws_url}/ws?session_id={session_id}&delivery=pull"))
        .header("authorization", format!("Bearer {token}"))
        .header("Host", base_url.trim_start_matches("http://"))
        .header("Upgrade", "websocket")
        .header("Connection", "upgrade")
        .header("Sec-WebSocket-Key", generate_key())
        .header("Sec-WebSocket-Version", "13")
        .body(())
        .map_err(|error| error.to_string())?;
    let (mut ws, _) = tokio_tungstenite::connect_async(request)
        .await
        .map_err(|error| error.to_string())?;
    let baseline = next_json(&mut ws).await?;
    println!(
        "{}",
        json!({ "stage": "baseline", "ok": baseline.get("Baseline").is_some() })
    );
    let mut signal = String::new();
    input
        .read_line(&mut signal)
        .await
        .map_err(|error| error.to_string())?;
    if signal.trim() != "advance-complete" {
        return Err("expected advance-complete signal".to_owned());
    }
    ws.send(Message::Text(r#"{"GetFrame":{}}"#.into()))
        .await
        .map_err(|error| error.to_string())?;
    let frame = next_named(&mut ws, "PublisherFrame").await?;
    println!(
        "{}",
        json!({ "stage": "frame", "ok": frame.get("PublisherFrame").is_some() })
    );
    signal.clear();
    input
        .read_line(&mut signal)
        .await
        .map_err(|error| error.to_string())?;
    if signal.trim() != "restore-complete" {
        return Err("expected restore-complete signal".to_owned());
    }
    let resync = next_named(&mut ws, "ResyncRequired").await?;
    ws.send(Message::Text(r#"{"Resync":{}}"#.into()))
        .await
        .map_err(|error| error.to_string())?;
    let fresh_baseline = next_named(&mut ws, "Baseline").await?;
    println!(
        "{}",
        json!({
            "stage": "complete",
            "resync": resync["ResyncRequired"]["reason"] == "timeline_changed",
            "freshBaseline": fresh_baseline.get("Baseline").is_some(),
        })
    );
    Ok(())
}
