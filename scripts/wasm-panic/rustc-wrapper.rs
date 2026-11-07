use std::convert::TryFrom;
use std::env;
use std::ffi::OsStr;
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(message) => {
            eprintln!("rustc wrapper: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<ExitCode, String> {
    let sysroot = env::var_os("WASM_PANIC_SYSROOT")
        .filter(|value| !value.is_empty())
        .ok_or("必须提供 WASM_PANIC_SYSROOT")?;
    let mut args = env::args_os().skip(1);
    let rustc = args.next().ok_or("必须提供 rustc 路径")?;
    let args: Vec<_> = args.collect();
    if args.iter().any(|arg| {
        arg == OsStr::new("--sysroot") || arg.to_string_lossy().starts_with("--sysroot=")
    }) {
        return Err("禁止覆盖 WASM_PANIC_SYSROOT".into());
    }
    let status = Command::new(rustc)
        .arg("--sysroot")
        .arg(sysroot)
        .args(args)
        .status()
        .map_err(|error| format!("无法运行 rustc：{error}"))?;
    Ok(status.code().and_then(|code| u8::try_from(code).ok()).map_or(ExitCode::FAILURE, ExitCode::from))
}
