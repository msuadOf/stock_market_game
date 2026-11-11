#[no_mangle]
pub extern "C" fn probe_current_unpark_park() {
    let thread = web_thread::current();
    thread.unpark();
    web_thread::park();
}
