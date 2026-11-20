# Contribution Guidelines

- (MUST) Write source code, comments, commit messages, GitHub issues, and committed documents in English
- (MUST) Use .gitmessage for commit template

## Development

### Requirements

- Nix 2.31.2 or later with flakes enabled
- One of the supported development platforms:
  - Linux on x86_64 or aarch64
  - macOS on Apple Silicon

Intel Macs cannot enter the development shell.
Intel builds are handled through a universal binary, tracked in issue #30.

### Development shell

Enter the development shell with `nix develop`, or let direnv load it through `.envrc`.
The shell provides the Rust toolchain pinned in `rust-toolchain.toml`, along with the pre-commit hooks.

### Checks

Run all checks at once, including the pre-commit checks:

```sh
nix flake check
```

Inside the development shell, you can run each check on its own:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Nix does not see files that git does not track.
Run `git add` on new files before running `nix flake check`.
