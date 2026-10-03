use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// 只拥有既有拒绝测试流程；不在 Drop 中新增清理政策。
struct RejectedCommandFixture {
    child: Option<Child>,
    deadline: Instant,
}

impl RejectedCommandFixture {
    fn spawn(arguments: &[&str]) -> Self {
        let child = Command::new(env!("CARGO_BIN_EXE_server"))
            .args(arguments)
            .env("STOCK_MARKET_GAME_SERVER_BIND_ADDR", "127.0.0.1:0")
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        Self {
            child: Some(child),
            deadline: Instant::now() + Duration::from_millis(500),
        }
    }

    fn kill_and_wait(&mut self) {
        let child = self.child.as_mut().expect("拒绝命令应仍拥有 child");
        child.kill().unwrap();
        child.wait().unwrap();
    }

    fn wait_rejected(mut self) -> String {
        loop {
            if self
                .child
                .as_mut()
                .expect("拒绝命令应仍拥有 child")
                .try_wait()
                .unwrap()
                .is_some()
            {
                let output = self
                    .child
                    .take()
                    .expect("拒绝命令应仍拥有 child")
                    .wait_with_output()
                    .unwrap();
                assert!(!output.status.success(), "invalid CLI must fail");
                return String::from_utf8(output.stderr).unwrap();
            }
            if Instant::now() >= self.deadline {
                self.kill_and_wait();
                panic!("invalid CLI was ignored instead of failing before listening");
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

fn rejected(arguments: &[&str]) -> String {
    RejectedCommandFixture::spawn(arguments).wait_rejected()
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
