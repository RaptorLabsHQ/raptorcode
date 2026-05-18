use std::env;
use std::io::{self, IsTerminal};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args = env::args().skip(1).collect::<Vec<_>>();
    let current_dir = match env::current_dir() {
        Ok(current_dir) => current_dir,
        Err(error) => {
            eprintln!("failed to read current directory: {error}");
            return ExitCode::FAILURE;
        }
    };

    let stdin = io::stdin();
    let stdin_interactive = stdin.is_terminal();
    let stdout_interactive = io::stdout().is_terminal();
    let startup_options =
        raptor_cli::runtime_startup_banner_options(&current_dir, stdout_interactive);

    if args.len() == 1
        && args[0] == "tui"
        && stdin_interactive
        && stdout_interactive
        && !raptor_cli::ci_environment_enabled()
    {
        return match raptor_cli::run_tui_dashboard(&current_dir) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("{error}");
                ExitCode::FAILURE
            }
        };
    }

    let result = if stdin_interactive {
        let mut stdin = stdin.lock();

        raptor_cli::run_with_startup_banner(args, &current_dir, Some(&mut stdin), startup_options)
    } else {
        raptor_cli::run_with_startup_banner(args, &current_dir, None, startup_options)
    };

    match result {
        Ok(output) => {
            print!("{output}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            if let Some(output) = error.output() {
                print!("{output}");
            }
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
