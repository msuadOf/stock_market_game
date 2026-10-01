use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn rejected(arguments: &[&str]) -> String {
    let mut child = Command::new(env!("CARGO_BIN_EXE_server"))
        .args(arguments)
        .env("STOCK_MARKET_GAME_SERVER_BIND_ADDR", "127.0.0.1:0")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_millis(500);
    loop {
        if child.try_wait().unwrap().is_some() {
            let output = child.wait_with_output().unwrap();
            assert!(!output.status.success(), "invalid CLI must fail");
            return String::from_utf8(output.stderr).unwrap();
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("invalid CLI was ignored instead of failing before listening");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn unknown_cli_argument_fails_explicitly() {
    assert!(rejected(&["--not-a-server-option"]).contains("--not-a-server-option"));
}

#[test]
fn invalid_services_fail_explicitly() {
    assert!(rejected(&["--services", "desktop"]).contains("--services"));
}

#[test]
fn missing_option_value_fails_explicitly() {
    assert!(rejected(&["--bind"]).contains("--bind"));
}

#[test]
fn web_root_cannot_hide_an_unknown_option() {
    assert!(rejected(&["--services", "server", "--web-root", "--typo"]).contains("--web-root"));
}

#[cfg(not(feature = "web-ui"))]
#[test]
fn server_only_build_rejects_static_modes() {
    for services in ["webui", "all"] {
        assert!(rejected(&["--services", services]).contains("web-ui"));
    }
}
