use std::process::ExitCode;

use openkeyboard_probe::cli_options::{CollectOptions, Command, USAGE, parse_arguments};
use openkeyboard_probe::console_window::wait_for_enter_if_started_from_explorer;
use openkeyboard_probe::device_report::write_device_report;
use openkeyboard_probe::report_collector::collect_device_report;
use openkeyboard_probe::summary::describe_report;

const USAGE_ERROR_EXIT_CODE: u8 = 2;

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let exit_code = run(&arguments);
    wait_for_enter_if_started_from_explorer();
    exit_code
}

fn run(arguments: &[String]) -> ExitCode {
    match parse_arguments(arguments) {
        Ok(Command::ShowHelp) => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Ok(Command::Collect(options)) => collect(&options),
        Err(message) => {
            eprintln!("{message}\n\n{USAGE}");
            ExitCode::from(USAGE_ERROR_EXIT_CODE)
        }
    }
}

fn collect(options: &CollectOptions) -> ExitCode {
    let saved_to = std::path::absolute(&options.output_path).unwrap_or_else(|_| options.output_path.clone());
    let written = collect_device_report(options.request.clone())
        .and_then(|report| write_device_report(&report, &saved_to).map(|()| report));
    match written {
        Ok(report) => {
            println!("{}", describe_report(&report, &saved_to));
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}
