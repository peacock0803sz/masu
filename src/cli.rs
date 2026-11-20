use clap::Parser;

#[derive(Debug, Parser)]
#[command(version, about, arg_required_else_help = true)]
pub struct Cli {}

#[cfg(test)]
mod tests {
    use super::Cli;
    use clap::CommandFactory;

    #[test]
    fn command_definition_is_valid() {
        Cli::command().debug_assert();
    }
}
