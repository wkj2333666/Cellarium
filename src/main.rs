//! Cellarium starts one window and runs its simulation in this process.

use std::error::Error;
use std::ffi::OsString;

use cellarium::cli::{CliMode, parse_cli};
use cellarium::gui::run::{GuiLaunchOptions, run};

fn main() {
    if let Err(error) = startup(std::env::args_os().skip(1)) {
        eprintln!("cellarium: {error}");
        std::process::exit(1);
    }
}

fn startup<I>(args: I) -> Result<(), Box<dyn Error>>
where
    I: IntoIterator<Item = OsString>,
{
    // A parse failure already carries its own explanation, including the usage
    // line where that is what the user needs.
    let options = parse_cli(args).map_err(CliError)?;
    match options.mode {
        CliMode::Version => {
            println!("cellarium {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        CliMode::Gui => Ok(run(GuiLaunchOptions {
            experiment: options.experiment,
            backend: options.backend,
            safe_mode: options.safe_mode,
        })?),
    }
}

#[derive(Debug)]
struct CliError(String);

impl std::fmt::Display for CliError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for CliError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_prints_and_returns_without_opening_a_window() {
        assert!(startup([OsString::from("--version")]).is_ok());
    }

    #[test]
    fn a_removed_mode_fails_with_its_own_explanation() {
        let error = startup([OsString::from("server")]).unwrap_err().to_string();
        assert!(error.contains("removed"), "{error}");
        assert!(
            !error.contains("usage:"),
            "a removed mode explains itself rather than printing the usage line"
        );
    }
}
