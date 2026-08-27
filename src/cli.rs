use std::ffi::OsString;
use std::path::PathBuf;

pub const USAGE: &str = "usage: cellarium [server | connect <host>] [--gui] [--kernel <path>] [--experiment <path>] [--save-experiment <path>]";

#[derive(Debug, Default, PartialEq, Eq)]
pub struct CliOptions {
    pub mode: CliMode,
    pub kernel: Option<PathBuf>,
    pub experiment: Option<PathBuf>,
    pub save_experiment: Option<PathBuf>,
    pub ssh_command: Option<String>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub enum CliMode {
    #[default]
    Direct,
    Gui,
    Version,
    Server,
    Connect {
        host: String,
    },
}

pub fn parse_cli<I>(args: I) -> Result<CliOptions, &'static str>
where
    I: IntoIterator<Item = OsString>,
{
    let mut options = CliOptions::default();
    let mut args = args.into_iter();
    while let Some(argument) = args.next() {
        let flag = argument.to_string_lossy();
        if flag == "--version" || flag == "-V" {
            if options != CliOptions::default() || args.next().is_some() {
                return Err("--version cannot be combined with other arguments");
            }
            options.mode = CliMode::Version;
            break;
        }
        if flag == "--gui" {
            if options.mode != CliMode::Direct {
                return Err("duplicate mode");
            }
            options.mode = CliMode::Gui;
            continue;
        }
        if flag == "server" {
            if options.mode != CliMode::Direct {
                return Err("duplicate mode");
            }
            options.mode = CliMode::Server;
            continue;
        }
        if flag == "connect" {
            if options.mode != CliMode::Direct {
                return Err("duplicate mode");
            }
            let host = args.next().ok_or("connect requires a host")?;
            options.mode = CliMode::Connect {
                host: host.to_string_lossy().into_owned(),
            };
            continue;
        }
        if flag == "--ssh-command" {
            if options.ssh_command.is_some() {
                return Err("duplicate argument");
            }
            let command = args.next().ok_or("--ssh-command requires a command")?;
            options.ssh_command = Some(command.to_string_lossy().into_owned());
            continue;
        }
        let target = match flag.as_ref() {
            "--kernel" => &mut options.kernel,
            "--experiment" => &mut options.experiment,
            "--save-experiment" => &mut options.save_experiment,
            _ => return Err("unexpected argument"),
        };
        if target.is_some() {
            return Err("duplicate argument");
        }
        let path = args.next().ok_or(match flag.as_ref() {
            "--kernel" => "--kernel requires a path",
            "--experiment" => "--experiment requires a path",
            _ => "--save-experiment requires a path",
        })?;
        *target = Some(PathBuf::from(path));
    }
    if options.kernel.is_some() && options.experiment.is_some() {
        return Err("--kernel and --experiment cannot be combined");
    }
    if options.ssh_command.is_some() && !matches!(options.mode, CliMode::Connect { .. }) {
        return Err("--ssh-command requires connect");
    }
    if matches!(options.mode, CliMode::Server)
        && (options.kernel.is_some()
            || options.experiment.is_some()
            || options.save_experiment.is_some())
    {
        return Err("server cannot be combined with direct-mode arguments");
    }
    if matches!(options.mode, CliMode::Gui)
        && (options.kernel.is_some() || options.save_experiment.is_some())
    {
        return Err("--gui accepts only --experiment");
    }
    Ok(options)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_parses_experiment_load_and_save_options() {
        let options = parse_cli([
            OsString::from("--experiment"),
            OsString::from("/tmp/input.ron"),
            OsString::from("--save-experiment"),
            OsString::from("/tmp/output.ron"),
        ])
        .unwrap();

        assert_eq!(options.experiment, Some(PathBuf::from("/tmp/input.ron")));
        assert_eq!(
            options.save_experiment,
            Some(PathBuf::from("/tmp/output.ron"))
        );
        assert_eq!(options.kernel, None);
    }

    #[test]
    fn cli_parses_standalone_version_flags_and_rejects_combinations() {
        assert_eq!(
            parse_cli([OsString::from("--version")]).unwrap().mode,
            CliMode::Version
        );
        assert_eq!(
            parse_cli([OsString::from("-V")]).unwrap().mode,
            CliMode::Version
        );
        assert_eq!(
            parse_cli([
                OsString::from("connect"),
                OsString::from("tinker"),
                OsString::from("--version")
            ])
            .unwrap_err(),
            "--version cannot be combined with other arguments"
        );
    }

    #[test]
    fn gui_mode_accepts_an_experiment_and_rejects_other_modes() {
        let options = parse_cli([
            OsString::from("--gui"),
            OsString::from("--experiment"),
            OsString::from("/tmp/input.ron"),
        ])
        .unwrap();
        assert_eq!(options.mode, CliMode::Gui);
        assert_eq!(options.experiment, Some(PathBuf::from("/tmp/input.ron")));

        assert_eq!(
            parse_cli([OsString::from("--gui"), OsString::from("server")]).unwrap_err(),
            "duplicate mode"
        );
        assert_eq!(
            parse_cli([
                OsString::from("--gui"),
                OsString::from("--kernel"),
                OsString::from("/tmp/kernel.ron")
            ])
            .unwrap_err(),
            "--gui accepts only --experiment"
        );
    }
}
