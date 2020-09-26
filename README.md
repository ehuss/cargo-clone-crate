# `cargo clone`

A Cargo subcommand to clone a repository from the
[crates.io](https://crates.io/) index.

## Installation

`cargo install cargo-clone-crate`

## Usage

By default it will attempt to guess if the package uses git, Mercurial, or
other version control systems.

`cargo clone bitflags`

If it can't determine which to use, you can force it manually:

`cargo clone --method=fossil rs-graph graph.fossil`

You can also download the `crate` file directly from crates.io:

`cargo clone --method=crate bitflags`

The `crate` method can also take a version to fetch a specific version:

`cargo clone --version=1.0.1 bitflags`

If passed a Cargo-style package spec with a version requirement, it will
always use the `crate` method to download directly from crates.io:

`cargo clone bitflags:^1.0`

Extra arguments are passed to the VCS command:

`cargo clone bitflags --depth=1 bf`
