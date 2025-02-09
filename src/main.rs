mod cli;
mod version;

fn main() {
    let matches = cli::build_cli().get_matches();

    if matches.subcommand().is_none() {
        println!("{}", cli::build_cli().render_long_help().to_string());
    }
}
