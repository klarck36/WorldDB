use std::collections::{BTreeMap, HashSet};
use std::env;
use std::path::PathBuf;
use std::process::{self, Command};
use std::time::Instant;

const MANIFEST_RELATIVE_PATH: &str = "tools/verify/steps.tsv";

type Result<T> = std::result::Result<T, String>;

#[derive(Debug, Clone, PartialEq, Eq)]
enum StepState {
    Run,
    Skip(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Step {
    id: String,
    state: StepState,
    program: Option<String>,
    args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Profile {
    name: String,
    is_default: bool,
    description: String,
    steps: Vec<Step>,
}

enum CliAction {
    Help,
    Verify {
        profile: Option<String>,
        skips: HashSet<String>,
    },
}

fn main() {
    if let Err(error) = run() {
        eprintln!("VERIFY ERROR: {error}");
        process::exit(1);
    }
}

fn run() -> Result<()> {
    let action = parse_cli(env::args().skip(1).collect())?;
    let CliAction::Verify {
        profile: cli_profile,
        skips,
    } = action
    else {
        print_help();
        return Ok(());
    };

    let root = find_repo_root(env::current_dir().map_err(|error| error.to_string())?)?;
    let manifest_path = root.join(MANIFEST_RELATIVE_PATH);
    let manifest = std::fs::read_to_string(&manifest_path)
        .map_err(|error| format!("cannot read {}: {error}", manifest_path.display()))?;
    let profiles = parse_manifest(&manifest)?;
    let requested_profile = cli_profile.or_else(|| {
        env::var("WORLDDB_VERIFY_PROFILE")
            .ok()
            .filter(|value| !value.is_empty())
    });
    let profile = select_profile(&profiles, requested_profile.as_deref())?;
    validate_requested_skips(profile, &skips)?;

    println!(
        "WorldDB verify profile: {} — {}",
        profile.name, profile.description
    );
    println!("Manifest: {}", manifest_path.display());
    let mut passed = 0usize;
    let mut skipped = 0usize;
    let mut failed = 0usize;

    for step in &profile.steps {
        match &step.state {
            StepState::Skip(reason) => {
                println!("[SKIP] {} — {}", step.id, reason);
                skipped += 1;
            }
            StepState::Run if skips.contains(&step.id) => {
                println!("[SKIP] {} — requested via --skip", step.id);
                skipped += 1;
            }
            StepState::Run => {
                let program = resolve_program(step.program.as_deref().unwrap_or_default());
                let command_line = std::iter::once(program.as_str())
                    .chain(step.args.iter().map(String::as_str))
                    .collect::<Vec<_>>()
                    .join(" ");
                println!("[RUN ] {}: {}", step.id, command_line);
                let start = Instant::now();
                let status = Command::new(&program)
                    .args(&step.args)
                    .current_dir(&root)
                    .status();
                match status {
                    Ok(status) if status.success() => {
                        println!("[PASS] {} ({:.2?})", step.id, start.elapsed());
                        passed += 1;
                    }
                    Ok(status) => {
                        eprintln!("[FAIL] {} (exit {})", step.id, status);
                        failed += 1;
                    }
                    Err(error) => {
                        eprintln!(
                            "[FAIL] {} (could not start {}: {})",
                            step.id, program, error
                        );
                        failed += 1;
                    }
                }
            }
        }
    }

    println!(
        "VERIFY SUMMARY: profile={} passed={} skipped={} failed={}",
        profile.name, passed, skipped, failed
    );
    if failed > 0 {
        return Err(format!("{failed} required verification step(s) failed"));
    }
    Ok(())
}

fn parse_cli(args: Vec<String>) -> Result<CliAction> {
    let Some(command) = args.first() else {
        return Ok(CliAction::Help);
    };
    if command == "--help" || command == "-h" || command == "help" {
        return Ok(CliAction::Help);
    }
    if command != "verify" {
        return Err(format!("unknown command {command:?}; expected `verify`"));
    }

    let mut profile = None;
    let mut skips = HashSet::new();
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--profile" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| "--profile requires a profile name".to_owned())?;
                if profile.replace(value.clone()).is_some() {
                    return Err("--profile may be specified only once".to_owned());
                }
            }
            "--skip" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| "--skip requires a step id".to_owned())?;
                if !skips.insert(value.clone()) {
                    return Err(format!(
                        "step {value:?} was listed more than once with --skip"
                    ));
                }
            }
            other => return Err(format!("unknown verify option {other:?}")),
        }
        index += 1;
    }

    Ok(CliAction::Verify { profile, skips })
}

fn find_repo_root(start: PathBuf) -> Result<PathBuf> {
    for candidate in start.ancestors() {
        if candidate.join("Cargo.toml").is_file()
            && candidate.join(MANIFEST_RELATIVE_PATH).is_file()
        {
            return Ok(candidate.to_path_buf());
        }
    }
    Err(format!(
        "could not find a repository root containing Cargo.toml and {MANIFEST_RELATIVE_PATH}"
    ))
}

fn parse_manifest(input: &str) -> Result<Vec<Profile>> {
    let mut profiles = BTreeMap::<String, Profile>::new();
    let mut default_profiles = 0usize;

    for (line_index, raw_line) in input.lines().enumerate() {
        let line = raw_line.trim_end_matches('\r');
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let fields = line.split('\t').collect::<Vec<_>>();
        match fields.first().copied() {
            Some("profile") if fields.len() == 4 => {
                let name = fields[1];
                let is_default = match fields[2] {
                    "default" => true,
                    "available" => false,
                    value => {
                        return Err(manifest_error(
                            line_index,
                            format!("invalid profile state {value:?}"),
                        ));
                    }
                };
                if name.is_empty() || fields[3].is_empty() {
                    return Err(manifest_error(
                        line_index,
                        "profile name and description must be non-empty".to_owned(),
                    ));
                }
                if profiles.contains_key(name) {
                    return Err(manifest_error(
                        line_index,
                        format!("duplicate profile {name:?}"),
                    ));
                }
                default_profiles += usize::from(is_default);
                profiles.insert(
                    name.to_owned(),
                    Profile {
                        name: name.to_owned(),
                        is_default,
                        description: fields[3].to_owned(),
                        steps: Vec::new(),
                    },
                );
            }
            Some("step") if fields.len() == 7 => {
                let profile_name = fields[1];
                let profile = profiles.get_mut(profile_name).ok_or_else(|| {
                    manifest_error(
                        line_index,
                        format!("step references unknown or undeclared profile {profile_name:?}"),
                    )
                })?;
                let id = fields[2];
                if id.is_empty() || profile.steps.iter().any(|step| step.id == id) {
                    return Err(manifest_error(
                        line_index,
                        format!("empty or duplicate step id {id:?} in profile {profile_name:?}"),
                    ));
                }
                let (state, program) = match fields[3] {
                    "run" => {
                        if fields[4].is_empty() || fields[6] != "-" {
                            return Err(manifest_error(
                                line_index,
                                "run steps require a program and '-' as their reason field"
                                    .to_owned(),
                            ));
                        }
                        (StepState::Run, Some(fields[4].to_owned()))
                    }
                    "skip" => {
                        if !fields[4].is_empty() || !fields[5].is_empty() || fields[6].is_empty() {
                            return Err(manifest_error(
                                line_index,
                                "skipped steps require empty command fields and a visible reason"
                                    .to_owned(),
                            ));
                        }
                        (StepState::Skip(fields[6].to_owned()), None)
                    }
                    value => {
                        return Err(manifest_error(
                            line_index,
                            format!("invalid step state {value:?}"),
                        ));
                    }
                };
                let args = fields[5]
                    .split_whitespace()
                    .map(str::to_owned)
                    .collect::<Vec<_>>();
                profile.steps.push(Step {
                    id: id.to_owned(),
                    state,
                    program,
                    args,
                });
            }
            Some(record) => {
                return Err(manifest_error(
                    line_index,
                    format!("unknown record or wrong field count for {record:?}"),
                ));
            }
            None => unreachable!("split always returns at least one field"),
        }
    }

    if profiles.is_empty() {
        return Err("verification manifest contains no profiles".to_owned());
    }
    if default_profiles != 1 {
        return Err(format!(
            "verification manifest must contain exactly one default profile; found {default_profiles}"
        ));
    }
    if profiles.values().any(|profile| profile.steps.is_empty()) {
        return Err("every verification profile must contain at least one step".to_owned());
    }
    Ok(profiles.into_values().collect())
}

fn manifest_error(line_index: usize, message: String) -> String {
    format!("manifest line {}: {message}", line_index + 1)
}

fn select_profile<'a>(profiles: &'a [Profile], requested: Option<&str>) -> Result<&'a Profile> {
    match requested {
        Some(name) => profiles
            .iter()
            .find(|profile| profile.name == name)
            .ok_or_else(|| format!("verification profile {name:?} is not defined")),
        None => profiles
            .iter()
            .find(|profile| profile.is_default)
            .ok_or_else(|| "verification manifest has no default profile".to_owned()),
    }
}

fn validate_requested_skips(profile: &Profile, skips: &HashSet<String>) -> Result<()> {
    for id in skips {
        match profile.steps.iter().find(|step| step.id == *id) {
            Some(Step {
                state: StepState::Run,
                ..
            }) => {}
            Some(_) => {
                return Err(format!(
                    "step {id:?} is already skipped by the manifest and cannot be skipped again"
                ));
            }
            None => {
                return Err(format!(
                    "step {id:?} is not part of profile {:?}",
                    profile.name
                ));
            }
        }
    }
    Ok(())
}

fn resolve_program(program: &str) -> String {
    if program != "python" {
        return program.to_owned();
    }
    env::var("WORLDDB_PYTHON")
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| {
            if cfg!(windows) {
                "python".to_owned()
            } else {
                "python3".to_owned()
            }
        })
}

fn print_help() {
    println!(
        "Usage: cargo xtask verify [--profile NAME] [--skip STEP_ID]\n\
         Runs the manifest-selected profile (default: dev). Skipped steps and reasons are always shown."
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_MANIFEST: &str = "\
profile\tdev\tdefault\tLocal checks\n\
profile\tci\tavailable\tContinuous integration checks\n\
step\tdev\tformat\trun\tcargo\tfmt --all -- --check\t-\n\
step\tdev\tdependency-audit\tskip\t\t\tAdded by M0-12.\n\
step\tci\tformat\trun\tcargo\tfmt --all -- --check\t-\n";

    #[test]
    fn manifest_exposes_default_profile_and_visible_skips() {
        let profiles = parse_manifest(VALID_MANIFEST).unwrap();
        let profile = select_profile(&profiles, None).unwrap();
        assert_eq!(profile.name, "dev");
        assert!(matches!(profile.steps[1].state, StepState::Skip(_)));
    }

    #[test]
    fn skipped_manifest_step_requires_a_reason() {
        let input = "profile\tdev\tdefault\tLocal checks\nstep\tdev\taudit\tskip\t\t\t\n";
        assert!(
            parse_manifest(input)
                .unwrap_err()
                .contains("visible reason")
        );
    }

    #[test]
    fn duplicate_step_ids_are_rejected() {
        let input = "profile\tdev\tdefault\tLocal checks\n\
step\tdev\tformat\trun\tcargo\tfmt --all\t-\n\
step\tdev\tformat\trun\tcargo\tfmt --all\t-\n";
        assert!(
            parse_manifest(input)
                .unwrap_err()
                .contains("duplicate step id")
        );
    }

    #[test]
    fn explicit_skip_must_name_a_required_step_in_the_selected_profile() {
        let profiles = parse_manifest(VALID_MANIFEST).unwrap();
        let profile = select_profile(&profiles, Some("dev")).unwrap();
        assert!(validate_requested_skips(profile, &HashSet::from(["format".to_owned()])).is_ok());
        assert!(validate_requested_skips(profile, &HashSet::from(["missing".to_owned()])).is_err());
        assert!(
            validate_requested_skips(profile, &HashSet::from(["dependency-audit".to_owned()]))
                .is_err()
        );
    }
}
