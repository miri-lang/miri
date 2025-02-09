use clap::Command;

use crate::version::version;

pub fn build_cli() -> Command {
    Command::new("miri")
        .version(version())
        .about("Miri Compiler")
        .author("Slavik Shynkarenko slavik@slavikdev.com")
        .arg_required_else_help(true)
}
