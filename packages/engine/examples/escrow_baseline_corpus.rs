#[path = "escrow_baseline/fee_control.rs"]
mod fee_control;
#[path = "escrow_baseline/frames.rs"]
mod frames;
#[cfg(test)]
#[path = "escrow_baseline/mapping_contract.rs"]
mod mapping_contract;
#[path = "escrow_baseline/scenarios.rs"]
mod scenarios;
#[path = "escrow_baseline/stress.rs"]
mod stress;
#[cfg(test)]
#[path = "escrow_baseline/tests.rs"]
mod tests;
#[path = "escrow_baseline/witnesses.rs"]
mod witnesses;

use std::{
    env,
    io::{self, Write},
    process,
};

#[derive(Debug, thiserror::Error)]
enum CorpusError {
    #[error(transparent)]
    Step(#[from] engine::session::StepFatal),
    #[error("invalid corpus arguments: {0}")]
    Arguments(String),
    #[error("engine boundary: {0}")]
    Engine(#[from] engine::SessionError),
    #[error("serialization: {0}")]
    Json(#[from] serde_json::Error),
    #[error("output: {0}")]
    Io(#[from] io::Error),
    #[error("corpus invariant: {0}")]
    Invariant(String),
}

fn run() -> Result<(), CorpusError> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args == ["--help"] {
        println!(
            "usage: escrow_baseline_corpus <equivalence|divergence-9|representation|stress> <seed>"
        );
        return Ok(());
    }
    let [name, raw_seed] = args.as_slice() else {
        return Err(CorpusError::Arguments(
            "expected scenario and seed".to_owned(),
        ));
    };
    let scenario = scenarios::Scenario::parse(name)?;
    let seed = raw_seed
        .parse::<u64>()
        .map_err(|error| CorpusError::Arguments(error.to_string()))?;
    let records = scenarios::collect(scenario, seed)?;
    let stdout = io::stdout();
    let mut output = io::BufWriter::new(stdout.lock());
    for record in records {
        serde_json::to_writer(&mut output, &record)?;
        writeln!(output)?;
    }
    output.flush()?;
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("escrow baseline failed: {error}");
        process::exit(2);
    }
}
