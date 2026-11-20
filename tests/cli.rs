use std::process::{Command, Output};

fn masu(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_masu"))
        .args(args)
        // Forced colors would put escape codes into the output we compare.
        .env_remove("CLICOLOR_FORCE")
        .output()
        .expect("failed to run masu")
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("stdout is not UTF-8")
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("stderr is not UTF-8")
}

#[test]
fn no_arguments_prints_help_to_stderr_and_exits_with_2() {
    let output = masu(&[]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(stderr(&output).contains("Usage: masu"));
}

fn assert_help(flag: &str) {
    let output = masu(&[flag]);
    assert_eq!(output.status.code(), Some(0));
    let out = stdout(&output);
    assert!(out.contains("Usage: masu"));
    assert!(out.contains(env!("CARGO_PKG_DESCRIPTION")));
    assert!(output.stderr.is_empty());
}

#[test]
fn long_help_prints_help_to_stdout() {
    assert_help("--help");
}

#[test]
fn short_help_prints_help_to_stdout() {
    assert_help("-h");
}

fn assert_version(flag: &str) {
    let output = masu(&[flag]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        stdout(&output),
        format!("masu {}\n", env!("CARGO_PKG_VERSION"))
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn long_version_prints_version_to_stdout() {
    assert_version("--version");
}

#[test]
fn short_version_prints_version_to_stdout() {
    assert_version("-V");
}

fn assert_usage_error(arg: &str) {
    let output = masu(&[arg]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(stderr(&output).starts_with("error:"));
}

#[test]
fn unknown_subcommand_is_a_usage_error() {
    assert_usage_error("frobnicate");
}

#[test]
fn unknown_option_is_a_usage_error() {
    assert_usage_error("--frobnicate");
}
