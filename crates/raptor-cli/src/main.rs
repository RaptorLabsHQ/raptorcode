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
    let result = if stdin.is_terminal() {
        let mut stdin = stdin.lock();

        raptor_cli::run_with_merge_confirmation(args, &current_dir, Some(&mut stdin))
    } else {
        raptor_cli::run_with_merge_confirmation(args, &current_dir, None)
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
