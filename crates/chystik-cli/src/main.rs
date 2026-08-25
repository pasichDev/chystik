mod args;
mod run;
mod tui;

use clap::Parser;
use std::ffi::OsString;

fn wants_machine_errors(arguments: &[OsString]) -> bool {
    let mut next_is_format = false;
    for argument in arguments.iter().skip(1).filter_map(|value| value.to_str()) {
        if next_is_format {
            return matches!(argument, "json" | "jsonl");
        }
        if argument == "--format" {
            next_is_format = true;
        } else if let Some(format) = argument.strip_prefix("--format=") {
            return matches!(format, "json" | "jsonl");
        }
    }
    false
}

fn main() {
    let arguments: Vec<OsString> = std::env::args_os().collect();
    let machine_errors = wants_machine_errors(&arguments);
    let cli = match args::Cli::try_parse_from(&arguments) {
        Ok(cli) => cli,
        Err(error) => {
            if machine_errors && error.exit_code() != 0 {
                run::write_machine_error(error.exit_code(), &error.to_string());
                std::process::exit(error.exit_code());
            }
            error.exit()
        }
    };
    match run::run(cli) {
        Ok(code) => std::process::exit(code.as_i32()),
        Err(error) => {
            if error.machine_output {
                run::write_machine_error(error.code.as_i32(), &error.message);
            } else {
                eprintln!("chystik: {}", error.message);
            }
            std::process::exit(error.code.as_i32());
        }
    }
}
