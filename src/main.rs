mod cli;
// Only tests call the scan module until the scan from issue #5 calls it from
// `main`. Gating it on `test` keeps `dead_code` denied instead of allowed.
// Issue #5 widens this to `#[cfg(any(target_os = "macos", test))]`.
#[cfg(test)]
mod scan;

use clap::Parser;

fn main() {
    let _cli = cli::Cli::parse();
}
