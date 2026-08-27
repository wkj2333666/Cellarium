use cellarium::cli::{CliMode, USAGE, parse_cli};
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
        format!("cellarium: unexpected argument\n{USAGE}")
    );
}

fn cli_error(message: &str) -> String {
    format!("cellarium: {message}\n{USAGE}")
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
        CliMode::Version => {
            println!("cellarium {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        CliMode::Server => return Ok(cellarium::app::run_server()?),
        CliMode::Connect { host } => {
            return Ok(cellarium::app::run_connect_with_command(
                host,
                options.ssh_command.as_deref(),
            )?);
        }
        CliMode::Gui => {
            return Ok(cellarium::gui::run(cellarium::gui::GuiLaunchOptions {
                experiment: options.experiment.clone(),
            })?);
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
