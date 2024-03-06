use clap::{Arg, ArgAction, Command};
use env_logger::{Builder, Target};
use std::{env, io::Write, process::exit};

#[macro_use]
extern crate log;
use log::LevelFilter;

fn start_logging() {
    // Start the logger
    let mut builder = Builder::from_default_env();

    // Enable logging and set custom output for the app if there is no other logging levels specified
    if env::var("RUST_LOG").is_err() {
        env::set_var("RUST_LOG", "cargo_clone");
        builder
            .target(Target::Stdout)
            .format(|buf, record| {
                // Simply write the line without any additional content
                writeln!(buf, "{}", record.args())
            })
            .filter(None, LevelFilter::Info);
    }

    builder.init();
}

fn main() {
    start_logging();

    let matches = Command::new("cargo-clone")
        .version(clap::crate_version!())
        .disable_version_flag(true)
        .bin_name("cargo")
        .subcommand_required(true)
        .propagate_version(true)
        .subcommand(
            Command::new("clone")
                .about("Clone a package from crates.io.")
                .allow_hyphen_values(true)
                .arg(
                    Arg::new("method")
                        .long("method")
                        .action(ArgAction::Set)
                        .value_parser(["crate", "git", "hg", "pijul", "fossil", "auto"])
                        .default_value("auto")
                        .help("Method to fetch package."),
                )
                .arg(
                    Arg::new("name")
                        .required(true)
                        .help("Package name to clone."),
                )
                .arg(
                    Arg::new("version")
                        .long("version")
                        .action(ArgAction::Set)
                        .help("Version to download."),
                )
                .arg(
                    Arg::new("extra")
                        .allow_hyphen_values(true)
                        .action(ArgAction::Append)
                        .help("Additional arguments passed to clone command."),
                ),
        )
        .get_matches();
    let submatches = matches
        .subcommand_matches("clone")
        .expect("Expected `clone` subcommand.");

    let method = submatches.get_one::<String>("method").unwrap();
    let name = submatches.get_one::<String>("name").unwrap();
    let version = submatches.get_one::<String>("version");
    let extra: Vec<&str> = submatches
        .get_many::<String>("extra")
        .map_or_else(Vec::new, |e| e.map(|x| x.as_str()).collect());

    let cloner = cargo_clone::Cloner::new();
    let result = cloner.clone(
        // UNWRAP: The argument parser should guarantee only sane values get passed here
        cargo_clone::CloneMethodKind::from(method).unwrap(),
        name,
        version.map(|x| x.as_str()),
        &extra,
    );
    if let Err(e) = result {
        error!("Error: {}", e);
        for cause in e.chain().skip(1) {
            error!("Caused by: {}", cause);
        }
        exit(1);
    }
    exit(0)
}
