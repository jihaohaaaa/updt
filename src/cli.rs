use clap::{Arg, Command as ClapCommand, builder::PossibleValuesParser};

use crate::state::TARGET_IDS;

pub enum CliCommand {
    Default,
    Update(Vec<String>),
    Fish,
}

pub fn parse_cli() -> CliCommand {
    let matches = ClapCommand::new("updt")
        .version(env!("CARGO_PKG_VERSION"))
        .subcommand(
            ClapCommand::new("update")
                .about("Update selected targets")
                .arg(
                    Arg::new("targets")
                        .num_args(1..)
                        .required(true)
                        .value_delimiter(',')
                        .value_parser(PossibleValuesParser::new(TARGET_IDS))
                        .help("Targets to update, for example: npm,cargo"),
                ),
        )
        .subcommand(
            ClapCommand::new("fish")
                .about("Install fish completion to ~/.config/fish/completions/updt.fish"),
        )
        .get_matches();

    match matches.subcommand() {
        Some(("update", sub)) => CliCommand::Update(
            sub.get_many::<String>("targets")
                .map(|v| v.cloned().collect())
                .unwrap_or_default(),
        ),
        Some(("fish", _)) => CliCommand::Fish,
        _ => CliCommand::Default,
    }
}
