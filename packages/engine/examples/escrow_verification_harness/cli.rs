use std::path::PathBuf;

pub const SCENARIO: &str = "task9-runtime-v1";
pub const USAGE: &str = "usage: escrow_verification_harness \
  --scenario task9-runtime-v1 --seed <u64> --budget <1|2|4|auto> \
  --repeat <u32> --mode <canonical|perturbed|negative-control> \
  [--disable-merge completion] --output <new-directory>";

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    Canonical,
    Perturbed,
    NegativeControl,
}

impl Mode {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "canonical" => Ok(Self::Canonical),
            "perturbed" => Ok(Self::Perturbed),
            "negative-control" => Ok(Self::NegativeControl),
            _ => Err(format!(
                "mode `{value}` is unsupported; expected canonical, perturbed, or negative-control"
            )),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MergeDimension {
    Completion,
}

impl MergeDimension {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "completion" => Ok(Self::Completion),
            _ => Err(format!(
                "disabled merge `{value}` is unsupported; expected completion"
            )),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Budget {
    One,
    Two,
    Four,
    Auto,
}

impl Budget {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "1" => Ok(Self::One),
            "2" => Ok(Self::Two),
            "4" => Ok(Self::Four),
            "auto" => Ok(Self::Auto),
            _ => Err(format!(
                "budget `{value}` is unsupported; expected 1, 2, 4, or auto"
            )),
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::One => "1",
            Self::Two => "2",
            Self::Four => "4",
            Self::Auto => "auto",
        }
    }

    pub fn thread_count(self) -> Result<usize, String> {
        match self {
            Self::One => Ok(1),
            Self::Two => Ok(2),
            Self::Four => Ok(4),
            Self::Auto => std::thread::available_parallelism()
                .map(usize::from)
                .map_err(|error| format!("cannot resolve auto thread budget: {error}")),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub scenario: String,
    pub seed: u64,
    pub budget: Budget,
    pub repeat: u32,
    pub mode: Mode,
    pub disabled_merge: Option<MergeDimension>,
    pub output: PathBuf,
}

pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Config, String> {
    let mut scenario = None;
    let mut seed = None;
    let mut budget = None;
    let mut repeat = None;
    let mut mode = None;
    let mut disabled_merge = None;
    let mut output = None;
    let mut args = args.into_iter();
    while let Some(flag) = args.next() {
        if flag == "-h" || flag == "--help" {
            return Err(USAGE.to_owned());
        }
        let value = args
            .next()
            .ok_or_else(|| format!("missing value for `{flag}`"))?;
        match flag.as_str() {
            "--scenario" if scenario.is_none() => scenario = Some(value),
            "--seed" if seed.is_none() => {
                seed = Some(
                    value
                        .parse::<u64>()
                        .map_err(|error| format!("seed `{value}` is not u64: {error}"))?,
                )
            }
            "--budget" if budget.is_none() => budget = Some(Budget::parse(&value)?),
            "--repeat" if repeat.is_none() => {
                repeat = Some(
                    value
                        .parse::<u32>()
                        .map_err(|error| format!("repeat `{value}` is not u32: {error}"))?,
                )
            }
            "--mode" if mode.is_none() => mode = Some(Mode::parse(&value)?),
            "--disable-merge" if disabled_merge.is_none() => {
                disabled_merge = Some(MergeDimension::parse(&value)?)
            }
            "--output" if output.is_none() => output = Some(PathBuf::from(value)),
            _ => return Err(format!("unknown or duplicate argument `{flag}`")),
        }
    }

    let scenario = scenario.ok_or_else(|| "--scenario is required".to_owned())?;
    if scenario != SCENARIO {
        return Err(format!(
            "scenario `{scenario}` is not frozen; expected `{SCENARIO}`"
        ));
    }
    let mode = mode.ok_or_else(|| "--mode is required".to_owned())?;
    let repeat = repeat.ok_or_else(|| "--repeat is required".to_owned())?;
    if repeat > 1 {
        return Err(format!(
            "repeat `{repeat}` is unsupported; the frozen matrix uses exactly 0 and 1"
        ));
    }
    match (mode, disabled_merge) {
        (Mode::NegativeControl, None) => {
            return Err("negative-control requires --disable-merge".to_owned())
        }
        (Mode::NegativeControl, Some(_)) | (_, None) => {}
        (_, Some(_)) => return Err("--disable-merge is only valid for negative-control".to_owned()),
    }
    Ok(Config {
        scenario,
        seed: seed.ok_or_else(|| "--seed is required".to_owned())?,
        budget: budget.ok_or_else(|| "--budget is required".to_owned())?,
        repeat,
        mode,
        disabled_merge,
        output: output.ok_or_else(|| "--output is required".to_owned())?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid() -> Vec<String> {
        "--scenario task9-runtime-v1 --seed 17 --budget 4 --repeat 1 --mode canonical --output capture"
            .split_whitespace()
            .map(str::to_owned)
            .collect()
    }

    #[test]
    fn accepts_the_frozen_cli_surface() {
        let config = parse(valid()).unwrap();
        assert_eq!(config.seed, 17);
        assert_eq!(config.budget, Budget::Four);
        assert_eq!(config.mode, Mode::Canonical);
    }

    #[test]
    fn validates_budget_and_negative_control_pairing() {
        let mut invalid_budget = valid();
        invalid_budget[5] = "3".to_owned();
        assert!(parse(invalid_budget).unwrap_err().contains("budget `3`"));

        let mut missing_dimension = valid();
        missing_dimension[9] = "negative-control".to_owned();
        assert!(parse(missing_dimension)
            .unwrap_err()
            .contains("requires --disable-merge"));
    }

    #[test]
    fn accepts_every_frozen_budget() {
        for (raw, expected) in [
            ("1", Budget::One),
            ("2", Budget::Two),
            ("4", Budget::Four),
            ("auto", Budget::Auto),
        ] {
            let mut args = valid();
            args[5] = raw.to_owned();
            assert_eq!(parse(args).unwrap().budget, expected);
        }
    }

    #[test]
    fn rejects_repeat_outside_the_frozen_matrix() {
        let mut args = valid();
        args[7] = "2".to_owned();
        assert!(parse(args).unwrap_err().contains("exactly 0 and 1"));
    }
}
