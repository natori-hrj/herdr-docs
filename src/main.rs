mod document;
mod setup;
mod ui;

use std::env;
use std::path::PathBuf;

fn main() {
    if let Err(error) = run() {
        eprintln!("herdr-docs: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let command = args.next().unwrap_or_else(|| "reader".to_string());

    match command.as_str() {
        "startup" => setup::run_startup(),
        "reader" | "open" => {
            let path = args.next().map(PathBuf::from);
            if args.next().is_some() {
                return Err("usage: herdr-docs reader [PATH]".to_string());
            }
            ui::run_reader(path)
        }
        "context" | "export" | "render" => {
            let Some(path) = args.next() else {
                return Err("usage: herdr-docs context PATH".to_string());
            };
            if args.next().is_some() {
                return Err("usage: herdr-docs context PATH".to_string());
            }
            let document = document::load_document(&PathBuf::from(path))?;
            print!("{}", document::context_text(&document));
            Ok(())
        }
        "doctor" => {
            if args.next().is_some() {
                return Err("usage: herdr-docs doctor".to_string());
            }
            document::print_doctor();
            Ok(())
        }
        "--version" | "-V" => {
            println!("herdr-docs {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        other => Err(format!(
            "unknown command '{other}'. Run `herdr-docs --help` for usage."
        )),
    }
}

fn print_help() {
    let lines = [
        "herdr-docs — a normalized document reader for Herdr",
        "",
        "Usage:",
        "  herdr-docs startup       Configure the Herdr keybinding after install",
        "  herdr-docs reader [PATH]  Open the TUI reader",
        "  herdr-docs context PATH   Print normalized document context",
        "  herdr-docs doctor         Show available document converters",
        "",
        "The Herdr pane starts in the current workspace. Use Tab to show files,",
        "o to open a path, / to search, c to copy context, and q to close.",
    ];
    println!("{}", lines.join("\n"));
}
