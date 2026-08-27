//! The command line of a local application.
//!
//! Cellarium runs its own simulation in its own window. There is no server to
//! start and no host to connect to, so the flags that used to select those are
//! kept only long enough to say they are gone: a user with an old script
//! deserves a sentence, not "unexpected argument".

use std::ffi::OsString;
use std::path::PathBuf;

use crate::sim::backend_selector::BackendPolicy;

pub const USAGE: &str = "usage: cellarium [--experiment <path>] [--backend auto|cuda|wgpu|cpu] [--safe-mode] [--version]";

#[derive(Debug, Default, PartialEq)]
pub struct CliOptions {
    pub mode: CliMode,
    pub experiment: Option<PathBuf>,
    pub backend: BackendPolicy,
    /// Start without probing a GPU, for a machine where that is what hangs.
    pub safe_mode: bool,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub enum CliMode {
    /// Open the window. This is what running Cellarium means.
    #[default]
    Gui,
    Version,
}

/// Modes that were removed, and what to say about each.
const REMOVED: [(&str, &str); 5] = [
    (
        "server",
        "remote/server mode was removed: Cellarium runs simulation locally in its own window",
    ),
    (
        "connect",
        "connect mode was removed: Cellarium runs simulation locally in its own window",
    ),
    (
        "--ssh-command",
        "--ssh-command was removed with connect mode: Cellarium runs simulation locally",
    ),
    (
        "--gui",
        "--gui was removed because the window is the only interface; run cellarium with no mode flag",
    ),
    (
        "--kernel",
        "--kernel was removed: open an experiment with --experiment, then edit its kernels in the window",
    ),
];

pub fn parse_cli<I>(args: I) -> Result<CliOptions, String>
where
    I: IntoIterator<Item = OsString>,
{
    let mut options = CliOptions::default();
    let mut args = args.into_iter();
    let mut saw_version = false;
    while let Some(argument) = args.next() {
        let flag = argument.to_string_lossy().into_owned();

        if let Some((_, message)) = REMOVED.iter().find(|(name, _)| *name == flag) {
            return Err((*message).to_string());
        }

        match flag.as_str() {
            "--version" | "-V" => {
                saw_version = true;
                options.mode = CliMode::Version;
            }
            "--safe-mode" => options.safe_mode = true,
            "--experiment" => {
                if options.experiment.is_some() {
                    return Err("--experiment was given twice".into());
                }
                let path = args
                    .next()
                    .ok_or_else(|| "--experiment requires a path".to_string())?;
                options.experiment = Some(PathBuf::from(path));
            }
            "--backend" => {
                let name = args
                    .next()
                    .ok_or_else(|| "--backend requires auto, cuda, wgpu or cpu".to_string())?;
                options.backend = parse_backend(&name.to_string_lossy())?;
            }
            "--help" | "-h" => return Err(USAGE.to_string()),
            other => return Err(format!("unexpected argument `{other}`\n{USAGE}")),
        }
    }
    if saw_version && (options.experiment.is_some() || options.safe_mode) {
        return Err("--version cannot be combined with other arguments".into());
    }
    Ok(options)
}

fn parse_backend(name: &str) -> Result<BackendPolicy, String> {
    match name {
        "auto" => Ok(BackendPolicy::Auto),
        "cuda" => Ok(BackendPolicy::RequireCuda),
        // A named adapter is chosen in the window, where the list of what this
        // machine actually has is visible.
        "wgpu" => Ok(BackendPolicy::RequireWgpu { adapter: None }),
        "cpu" => Ok(BackendPolicy::RequireCpu),
        other => Err(format!(
            "unknown backend `{other}`: expected auto, cuda, wgpu or cpu"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<CliOptions, String> {
        parse_cli(args.iter().map(OsString::from))
    }

    #[test]
    fn running_with_no_arguments_opens_the_window() {
        let options = parse(&[]).unwrap();
        assert_eq!(options.mode, CliMode::Gui);
        assert_eq!(options.backend, BackendPolicy::Auto);
        assert!(!options.safe_mode);
    }

    #[test]
    fn remote_modes_are_removed_with_an_actionable_error() {
        let error = parse(&["server"]).unwrap_err();
        assert!(error.contains("remote/server mode was removed"), "{error}");

        let error = parse(&["connect", "tinker"]).unwrap_err();
        assert!(error.contains("runs simulation locally"), "{error}");

        let error = parse(&["--ssh-command", "ssh tinker"]).unwrap_err();
        assert!(error.contains("removed"), "{error}");
    }

    #[test]
    fn the_temporary_gui_flag_is_removed_and_says_what_to_run_instead() {
        let error = parse(&["--gui"]).unwrap_err();
        assert!(error.contains("no mode flag"), "{error}");
    }

    #[test]
    fn every_removed_flag_explains_itself_rather_than_being_unexpected() {
        for (flag, _) in REMOVED {
            let error = parse(&[flag]).unwrap_err();
            assert!(
                !error.contains("unexpected argument"),
                "{flag} deserves an explanation, got: {error}"
            );
            assert!(error.contains("removed"), "{flag}: {error}");
        }
    }

    #[test]
    fn a_backend_can_be_chosen_by_name() {
        assert_eq!(
            parse(&["--backend", "cpu"]).unwrap().backend,
            BackendPolicy::RequireCpu
        );
        assert_eq!(
            parse(&["--backend", "cuda"]).unwrap().backend,
            BackendPolicy::RequireCuda
        );
        assert_eq!(
            parse(&["--backend", "wgpu"]).unwrap().backend,
            BackendPolicy::RequireWgpu { adapter: None }
        );
        assert_eq!(
            parse(&["--backend", "auto"]).unwrap().backend,
            BackendPolicy::Auto
        );
    }

    #[test]
    fn an_unknown_backend_lists_the_ones_that_exist() {
        let error = parse(&["--backend", "metal"]).unwrap_err();
        assert!(error.contains("metal"), "{error}");
        assert!(error.contains("auto, cuda, wgpu or cpu"), "{error}");
    }

    #[test]
    fn an_experiment_path_is_accepted_once() {
        let options = parse(&["--experiment", "/tmp/a.ron"]).unwrap();
        assert_eq!(options.experiment, Some(PathBuf::from("/tmp/a.ron")));
        assert!(parse(&["--experiment"]).is_err(), "a path is required");
        assert!(
            parse(&["--experiment", "/a.ron", "--experiment", "/b.ron"]).is_err(),
            "two paths would leave it ambiguous which one is open"
        );
    }

    #[test]
    fn safe_mode_is_a_plain_flag() {
        assert!(parse(&["--safe-mode"]).unwrap().safe_mode);
    }

    #[test]
    fn version_stands_alone() {
        assert_eq!(parse(&["--version"]).unwrap().mode, CliMode::Version);
        assert_eq!(parse(&["-V"]).unwrap().mode, CliMode::Version);
        assert!(parse(&["--version", "--safe-mode"]).is_err());
    }

    #[test]
    fn help_prints_the_usage_line() {
        let error = parse(&["--help"]).unwrap_err();
        assert_eq!(error, USAGE);
        assert!(
            !USAGE.contains("server"),
            "the usage must not advertise a server"
        );
        assert!(
            !USAGE.contains("connect"),
            "the usage must not advertise connect"
        );
    }

    #[test]
    fn an_unknown_argument_shows_the_usage() {
        let error = parse(&["--frobnicate"]).unwrap_err();
        assert!(error.contains("--frobnicate"), "{error}");
        assert!(error.contains("usage:"), "{error}");
    }
}
