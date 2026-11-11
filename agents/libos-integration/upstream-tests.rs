mod watchdog {
    use std::time::Duration;
    #[derive(Clone, Copy, Debug)] pub enum Phase { TickConverge }
    pub struct WatchdogConfig;
    impl Default for WatchdogConfig { fn default() -> Self { Self } }
    impl WatchdogConfig { pub fn limit_for(&self, _: Phase) -> Duration { Duration::from_secs(1) } }
    pub fn recv_deadline<T>(rx: &std::sync::mpsc::Receiver<T>, _: Phase, _: &WatchdogConfig) -> Result<T, super::protocol::RuntimeError> { Ok(rx.recv().unwrap()) }
}
mod protocol {
    use super::watchdog::Phase;
    use std::time::Duration;
    #[derive(Debug)] pub enum RuntimeError { Watchdog { phase: Phase, waited: Duration, detail: String } }
}
#[path = "../../packages/engine/src/runtime/upstream.rs"] mod upstream;
