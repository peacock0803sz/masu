{
  description = "masu";

  inputs = {
    flake-parts.url = "github:hercules-ci/flake-parts";
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    git-hooks = {
      url = "github:cachix/git-hooks.nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    crane.url = "github:ipetkov/crane";
  };

  outputs =
    inputs@{ flake-parts, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      imports = [
        inputs.git-hooks.flakeModule
      ];
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
      ];
      perSystem =
        {
          config,
          pkgs,
          system,
          lib,
          ...
        }:
        let
          toolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
          craneLib = (inputs.crane.mkLib pkgs).overrideToolchain (
            p: p.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml
          );

          commonArgs = {
            src = craneLib.cleanCargoSource ./.;
            strictDeps = true;
            CARGO_PROFILE = "";
          }
          // lib.optionalAttrs (system == "aarch64-darwin") {
            buildInputs = [ pkgs.libiconv ];
          };

          cargoArtifacts = craneLib.buildDepsOnly commonArgs;

          releaseArgs = commonArgs // {
            CARGO_PROFILE = "release";
          };
        in
        {
          _module.args.pkgs = import inputs.nixpkgs {
            inherit system;
            overlays = [ inputs.rust-overlay.overlays.default ];
          };

          checks = {
            masu-fmt = craneLib.cargoFmt { inherit (commonArgs) src; };
            masu-clippy = craneLib.cargoClippy (
              commonArgs
              // {
                inherit cargoArtifacts;
                cargoClippyExtraArgs = "--all-targets -- --deny warnings";
              }
            );
            masu-test = craneLib.cargoTest (commonArgs // { inherit cargoArtifacts; });
          };

          packages.default = craneLib.buildPackage (
            releaseArgs
            // {
              cargoArtifacts = craneLib.buildDepsOnly releaseArgs;
              doCheck = false;
            }
          );

          pre-commit.settings.hooks = {
            nixfmt.enable = true;
            rustfmt = {
              enable = true;
              packageOverrides = {
                cargo = toolchain;
                rustfmt = toolchain;
              };
            };
          };

          devShells.default = craneLib.devShell {
            checks = config.checks;
            inputsFrom = [ config.pre-commit.devShell ];
            packages = [
              pkgs.cargo-insta
              pkgs.jq
            ];
          };
        };
      flake = { };
    };
}
