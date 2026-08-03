use std::{env, fs, path::PathBuf, process::ExitCode};

use sekai::generators::natural::circulation::{
    run_comparison_suite, CirculationFixture, ComparisonSuiteReport,
};

const DEFAULT_RESOLUTIONS: &[u16] = &[12, 24, 32];
const DEFAULT_SAMPLES: usize = 9;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("circulation_compare: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let arguments = parse_arguments(env::args().skip(1))?;
    let fixtures = [
        CirculationFixture::AquaPlanet,
        CirculationFixture::TwoBasins,
        CirculationFixture::EarthLikeHarmonics,
    ];
    let report = run_comparison_suite(&arguments.resolutions, &fixtures, arguments.samples)
        .map_err(|error| error.to_string())?;
    print_table(&report);
    if let Some(path) = arguments.json_path {
        let json = serde_json::to_vec_pretty(&report)
            .map_err(|error| format!("could not serialize JSON report: {error}"))?;
        fs::write(&path, json)
            .map_err(|error| format!("could not write {}: {error}", path.display()))?;
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Arguments {
    resolutions: Vec<u16>,
    samples: usize,
    json_path: Option<PathBuf>,
}

fn parse_arguments(arguments: impl IntoIterator<Item = String>) -> Result<Arguments, String> {
    let mut resolutions = None;
    let mut samples = None;
    let mut json_path = None;
    let mut arguments = arguments.into_iter();
    while let Some(flag) = arguments.next() {
        let value = arguments
            .next()
            .ok_or_else(|| format!("missing value after {flag}"))?;
        match flag.as_str() {
            "--resolutions" => {
                if resolutions.is_some() {
                    return Err("--resolutions was supplied more than once".to_owned());
                }
                resolutions = Some(parse_resolutions(&value)?);
            }
            "--samples" => {
                if samples.is_some() {
                    return Err("--samples was supplied more than once".to_owned());
                }
                samples = Some(
                    value
                        .parse::<usize>()
                        .map_err(|_| format!("invalid --samples value {value:?}"))?,
                );
            }
            "--json" => {
                if json_path.is_some() {
                    return Err("--json was supplied more than once".to_owned());
                }
                if value.is_empty() {
                    return Err("--json path cannot be empty".to_owned());
                }
                json_path = Some(PathBuf::from(value));
            }
            _ => return Err(format!("unknown flag {flag:?}")),
        }
    }
    Ok(Arguments {
        resolutions: resolutions.unwrap_or_else(|| DEFAULT_RESOLUTIONS.to_vec()),
        samples: samples.unwrap_or(DEFAULT_SAMPLES),
        json_path,
    })
}

fn parse_resolutions(value: &str) -> Result<Vec<u16>, String> {
    if value.is_empty() {
        return Err("--resolutions list cannot be empty".to_owned());
    }
    value
        .split(',')
        .map(|part| {
            if part.is_empty() {
                return Err(format!("invalid --resolutions list {value:?}"));
            }
            part.parse::<u16>()
                .map_err(|_| format!("invalid resolution {part:?}"))
        })
        .collect()
}

fn print_table(report: &ComparisonSuiteReport) {
    println!(
        "{:<20} {:>4} {:>7} {:>12} {:>14} {:>14} {:>10}",
        "fixture", "n", "cells", "steady ms", "cold trans ms", "warm trans ms", "WYSIWYG"
    );
    for case in &report.cases {
        println!(
            "{:<20} {:>4} {:>7} {:>12.3} {:>14.3} {:>14.3} {:>10}",
            fixture_name(case.fixture),
            case.face_resolution,
            case.cell_count,
            nanoseconds_to_milliseconds(case.timings.steady_solve.median_ns),
            nanoseconds_to_milliseconds(case.timings.transient_cold_solve.median_ns),
            nanoseconds_to_milliseconds(case.timings.transient_warm_solve.median_ns),
            if case.comparison.wysiwyg.eligible {
                "pass"
            } else {
                "fail"
            },
        );
    }
}

const fn fixture_name(fixture: CirculationFixture) -> &'static str {
    match fixture {
        CirculationFixture::AquaPlanet => "aqua-planet",
        CirculationFixture::TwoBasins => "two-basins",
        CirculationFixture::EarthLikeHarmonics => "earth-like",
    }
}

fn nanoseconds_to_milliseconds(value: u64) -> f64 {
    value as f64 / 1_000_000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_supported_flags_and_rejects_unknown_or_malformed_values() {
        let parsed = parse_arguments([
            "--resolutions".to_owned(),
            "4,8".to_owned(),
            "--samples".to_owned(),
            "3".to_owned(),
            "--json".to_owned(),
            "report.json".to_owned(),
        ])
        .unwrap();
        assert_eq!(parsed.resolutions, vec![4, 8]);
        assert_eq!(parsed.samples, 3);
        assert_eq!(parsed.json_path, Some(PathBuf::from("report.json")));

        assert!(parse_arguments(["--other".to_owned(), "1".to_owned()]).is_err());
        assert!(parse_arguments(["--samples".to_owned(), "x".to_owned()]).is_err());
        assert!(parse_arguments(["--resolutions".to_owned(), "4,,8".to_owned()]).is_err());
    }
}
