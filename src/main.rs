use cargo_clone;
use clap::{App, AppSettings, Arg, SubCommand};
use std::process::exit;

fn main() {
    let matches = App::new("cargo-clone")
        .version(clap::crate_version!())
        .bin_name("cargo")
        .setting(AppSettings::GlobalVersion)
        .setting(AppSettings::SubcommandRequired)
        .setting(AppSettings::ColoredHelp)
        .subcommand(
            SubCommand::with_name("clone")
                .about("Clone a package from crates.io.")
                .setting(AppSettings::AllowLeadingHyphen)
                .setting(AppSettings::ColoredHelp)
                .arg(
                    Arg::with_name("method")
                        .long("method")
                        .takes_value(true)
                        .possible_values(&["crate", "git", "hg", "pijul", "fossil", "auto"])
                        .default_value("auto")
                        .help("Method to fetch package."),
                )
                .arg(
                    Arg::with_name("name")
                        .required(true)
                        .help("Package name to clone."),
                )
                .arg(
                    Arg::with_name("version")
                        .long("version")
                        .takes_value(true)
                        .help("Version to download."),
                )
                .arg(
                    Arg::with_name("extra")
                        .allow_hyphen_values(true)
                        .multiple(true)
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

    let result = cargo_clone::clone(method, name, version, &extra);
    if let Err(e) = result {
        eprintln!("Error: {}", e);
        for cause in e.iter_causes() {
            eprintln!("Caused by: {}", cause);
        }
        exit(1);
    }
    exit(0)
}
