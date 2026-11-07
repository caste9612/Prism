//! Prism development CLI. Not a public product: it exists to run scans,
//! benchmarks and diagnostics without the GUI (docs/ARCHITECTURE.md §2).

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None | Some("-h" | "--help") => {
            print_help();
            ExitCode::SUCCESS
        }
        Some("-V" | "--version") => {
            println!("prism-cli {}", prism_core::VERSION);
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("unknown command: {other}\n");
            print_help();
            ExitCode::from(2)
        }
    }
}

fn print_help() {
    println!(
        "prism-cli {} - Prism development tool\n\n\
         USAGE:\n    prism-cli [--help | --version]\n\n\
         Commands (scan, ls, du, stats, bench) arrive in milestone T1.",
        prism_core::VERSION
    );
}
