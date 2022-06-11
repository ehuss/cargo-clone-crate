use cargo_clone;
use clap::{Arg, Command};
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
                        .takes_value(true)
                        .possible_values(&["crate", "git", "hg", "pijul", "fossil", "auto"])
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
                        .takes_value(true)
                        .help("Version to download."),
                )
                .arg(
                    Arg::new("extra")
                        .allow_hyphen_values(true)
                        .multiple_occurrences(true)
                        .help("Additional arguments passed to clone command."),
                ),
        )
        .get_matches();
    let submatches = matches
        .subcommand_matches("clone")
        .expect("Expected `clone` subcommand.");

    let method = submatches.value_of("method").unwrap();
    let name = submatches.value_of("name").unwrap();
    let version = submatches.value_of("version");
    let extra: Vec<&str> = submatches
        .values_of("extra")
        .map_or_else(Vec::new, |e| e.collect());

    let cloner = cargo_clone::Cloner::new();
    let result = cloner.clone(
        // UNWRAP: The argument parser should guarantee only sane values get passed here
        cargo_clone::CloneMethodKind::from(method).unwrap(),
        name,
        version,
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
