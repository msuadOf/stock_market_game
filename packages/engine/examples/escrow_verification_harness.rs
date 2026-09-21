//! Task 9 public-runtime capture harness.
//!
//! This executable intentionally exits with a typed `BLOCKED` report while the
//! public `GameSession::step` authority still uses the legacy compatibility
//! bridge or committed receipt/finalizer/perturbation evidence is unavailable.
//! Its non-receipt artifacts are real `ProtocolSession` output; they are not a
//! substitute for final escrow-pipeline acceptance evidence.

#[path = "escrow_verification_harness/cli.rs"]
mod cli;
#[path = "escrow_verification_harness/digest.rs"]
mod digest;
#[path = "escrow_verification_harness/runtime.rs"]
mod runtime;

use std::{
    env,
    ffi::OsString,
    io::{self, Write},
    path::PathBuf,
    process,
};

fn main() {
    let outcome = {
        let stdout = io::stdout();
        let mut stdout = stdout.lock();
        run(&mut stdout)
    };
    match outcome {
        Ok(outcome) => process::exit(outcome.exit_code()),
        Err(error) => {
            let stderr = io::stderr();
            let mut stderr = stderr.lock();
            let _ = writeln!(
                stderr,
                "escrow verification harness failed: {error}\n\n{}",
                cli::USAGE
            );
            process::exit(2);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RunOutcome {
    EvidenceBlocked,
}

impl RunOutcome {
    const fn exit_code(self) -> i32 {
        match self {
            Self::EvidenceBlocked => 3,
        }
    }
}

fn run(stdout: &mut impl Write) -> Result<RunOutcome, String> {
    let config = cli::parse(parse_os_args(env::args_os().skip(1))?)?;
    let paths = runtime::validate_workspace_paths(&config)?;
    let output = paths
        .get("OUTPUT")
        .map(PathBuf::from)
        .ok_or_else(|| "validated path report omitted OUTPUT".to_owned())?;
    let bundle = runtime::execute(&config)?;
    runtime::write_bundle(&bundle, &output)?;
    let summary = serde_json::to_string_pretty(&serde_json::json!({
        "status": "BLOCKED",
        "capture": output.join("capture.json"),
        "paths": paths,
        "reason": "formal escrow cutover and committed receipt/finalizer/perturbation evidence seams are not public",
    }))
    .map_err(|error| format!("stdout summary serialization failed: {error}"))?;
    write_stdout_summary(stdout, &summary)?;
    Ok(RunOutcome::EvidenceBlocked)
}

fn parse_os_args(args: impl IntoIterator<Item = OsString>) -> Result<Vec<String>, String> {
    args.into_iter()
        .map(|argument| {
            argument
                .into_string()
                .map_err(|_| "command-line argument is not valid UTF-8".to_owned())
        })
        .collect()
}

fn write_stdout_summary(stdout: &mut impl Write, summary: &str) -> Result<(), String> {
    writeln!(stdout, "{summary}").map_err(|error| format!("cannot write stdout summary: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct BrokenWriter;

    impl Write for BrokenWriter {
        fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed reader"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn blocked_capture_can_never_exit_as_pass() {
        assert_eq!(RunOutcome::EvidenceBlocked.exit_code(), 3);
        assert_ne!(RunOutcome::EvidenceBlocked.exit_code(), 0);
        assert_ne!(RunOutcome::EvidenceBlocked.exit_code(), 2);
    }

    #[test]
    fn stdout_broken_pipe_is_a_typed_io_error() {
        let error = write_stdout_summary(&mut BrokenWriter, "{}").unwrap_err();
        assert!(error.contains("cannot write stdout summary"));
        assert!(error.contains("closed reader"));
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_process_argument_is_a_typed_argument_error() {
        use std::os::unix::ffi::OsStringExt;

        let error = parse_os_args([OsString::from_vec(vec![0xff])]).unwrap_err();
        assert!(error.contains("not valid UTF-8"));
    }
}
