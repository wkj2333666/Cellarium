use cellarium::sim::kernel_file::load_kernel;
use std::error::Error;
use std::ffi::OsString;
use std::path::PathBuf;

fn main() {
    if let Err(error) = startup(std::env::args_os().skip(1)) {
        print_error(error.as_ref());
        std::process::exit(1);
    }
}

#[test]
fn kernel_file_cli_errors_include_concise_usage() {
    assert_eq!(
        cli_error("unexpected argument"),
        "cellarium: unexpected argument\nusage: cellarium [server | connect <host>] [--kernel <path>] [--experiment <path>] [--save-experiment <path>]"
    );
}

fn cli_error(message: &str) -> String {
    format!(
        "cellarium: {message}\nusage: cellarium [server | connect <host>] [--kernel <path>] [--experiment <path>] [--save-experiment <path>]"
    )
}

#[derive(Debug)]
struct CliError(&'static str);

impl std::fmt::Display for CliError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}

impl Error for CliError {}

fn print_error(error: &(dyn Error + 'static)) {
    if let Some(error) = error.downcast_ref::<CliError>() {
        eprintln!("{}", cli_error(error.0));
    } else {
        eprintln!("cellarium: {error}");
    }
}

fn startup<I>(args: I) -> Result<(), Box<dyn Error>>
where
    I: IntoIterator<Item = OsString>,
{
    let options = parse_cli(args).map_err(CliError)?;
    match &options.mode {
        CliMode::Server => return Ok(cellarium::app::run_server()?),
        CliMode::Connect { host } => {
            return Ok(cellarium::app::run_connect_with_command(
                host,
                options.ssh_command.as_deref(),
            )?);
        }
        CliMode::Direct => {}
    }
    if let Some(experiment_path) = options.experiment {
        let file = cellarium::sim::experiment::load_experiment(&experiment_path)?;
        if let Some(save_path) = options.save_experiment {
            return Ok(cellarium::app::run_with_experiment_and_save(
                file, save_path,
            )?);
        }
        return Ok(cellarium::app::run_with_experiment(file)?);
    }
    if let Some(kernel_path) = options.kernel {
        let definition = load_kernel(&kernel_path)?;
        if let Some(save_path) = options.save_experiment {
            return Ok(cellarium::app::run_with_kernel_and_save(
                definition, save_path,
            )?);
        }
        return Ok(cellarium::app::run_with_kernel(definition)?);
    }
    if let Some(save_path) = options.save_experiment {
        return Ok(cellarium::app::run_with_save(save_path)?);
    }
    Ok(cellarium::app::run()?)
}

#[derive(Debug, Default, PartialEq, Eq)]
struct CliOptions {
    mode: CliMode,
    kernel: Option<PathBuf>,
    experiment: Option<PathBuf>,
    save_experiment: Option<PathBuf>,
    ssh_command: Option<String>,
}

#[derive(Debug, Default, PartialEq, Eq)]
enum CliMode {
    #[default]
    Direct,
    Server,
    Connect {
        host: String,
    },
}

fn parse_cli<I>(args: I) -> Result<CliOptions, &'static str>
where
    I: IntoIterator<Item = OsString>,
{
    let mut options = CliOptions::default();
    let mut args = args.into_iter();
    while let Some(argument) = args.next() {
        let flag = argument.to_string_lossy();
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
    Ok(options)
}

#[allow(dead_code)]
fn parse_kernel_path<I>(args: I) -> Result<Option<PathBuf>, &'static str>
where
    I: IntoIterator<Item = OsString>,
{
    let mut args = args.into_iter();
    let Some(argument) = args.next() else {
        return Ok(None);
    };
    if argument != "--kernel" {
        return Err("unexpected argument");
    }

    let path = args.next().ok_or("--kernel requires a path")?;
    if args.next().is_some() {
        return Err("unexpected argument");
    }
    Ok(Some(PathBuf::from(path)))
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
    fn kernel_file_cli_parses_the_exact_flag_and_path() {
        let path = PathBuf::from("/tmp/cellarium-example.ron");

        assert_eq!(
            parse_kernel_path([OsString::from("--kernel"), OsString::from(&path)]).unwrap(),
            Some(path)
        );
        assert_eq!(parse_kernel_path(Vec::<OsString>::new()).unwrap(), None);
    }

    #[test]
    fn kernel_file_cli_rejects_malformed_arguments_with_usage() {
        let missing_path = parse_kernel_path([OsString::from("--kernel")]).unwrap_err();
        assert_eq!(missing_path, "--kernel requires a path");

        let unknown_argument = parse_kernel_path([OsString::from("--kernelx")]).unwrap_err();
        assert_eq!(unknown_argument, "unexpected argument");

        let trailing_argument = parse_kernel_path([
            OsString::from("--kernel"),
            OsString::from("/tmp/example.ron"),
            OsString::from("extra"),
        ])
        .unwrap_err();
        assert_eq!(trailing_argument, "unexpected argument");
    }
}
