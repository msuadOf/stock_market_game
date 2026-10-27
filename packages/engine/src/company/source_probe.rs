//! 仅测试的当前线程调用观测；不进入运行态、存档、RNG 或生产分支。
use std::cell::Cell;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct Counts {
    pub legacy_assembly: u32,
    pub legacy_prehistory: u32,
    pub legacy_advance: u32,
    pub simple_advance: u32,
    pub simple_finalize: u32,
}

thread_local! { static COUNTS: Cell<Counts> = Cell::new(Counts::default()); }

pub(crate) enum Call {
    LegacyAssembly,
    LegacyPrehistory,
    LegacyAdvance,
    SimpleAdvance,
    SimpleFinalize,
}

pub(crate) fn record(call: Call) {
    COUNTS.with(|cell| {
        let mut counts = cell.get();
        let counter = match call {
            Call::LegacyAssembly => &mut counts.legacy_assembly,
            Call::LegacyPrehistory => &mut counts.legacy_prehistory,
            Call::LegacyAdvance => &mut counts.legacy_advance,
            Call::SimpleAdvance => &mut counts.simple_advance,
            Call::SimpleFinalize => &mut counts.simple_finalize,
        };
        *counter = counter.checked_add(1).expect("测试调用计数不得溢出");
        cell.set(counts);
    });
}

pub(crate) fn reset() {
    COUNTS.with(|cell| cell.set(Counts::default()));
}
pub(crate) fn snapshot() -> Counts {
    COUNTS.with(Cell::get)
}
