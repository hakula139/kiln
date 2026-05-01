# ==============================================================================
# kiln Development Flake
# ==============================================================================
#
# Reproducible dev shell with the Rust toolchain, libdav1d (for `image`'s
# AVIF decoder), and the build-time tooling kiln shells out to (`pagefind`).
#
# Usage:
#
#   nix develop                            # interactive shell
#   nix develop --command cargo build      # one-shot
#   nix flake check                        # run pre-commit hooks

{
  description = "kiln — custom static site generator (dev environment)";

  # ----------------------------------------------------------------------------
  # Inputs
  # ----------------------------------------------------------------------------
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.11";

    flake-utils.url = "github:numtide/flake-utils";

    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    git-hooks-nix.url = "github:cachix/git-hooks.nix";
  };

  # ----------------------------------------------------------------------------
  # Outputs
  # ----------------------------------------------------------------------------
  outputs =
    {
      nixpkgs,
      flake-utils,
      rust-overlay,
      git-hooks-nix,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
        };

        # Stable Rust with components used by `cargo clippy`, coverage, and
        # editor tooling.
        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [
            "llvm-tools-preview"
            "rust-analyzer"
            "rust-src"
          ];
        };

        # ----------------------------------------------------------------------
        # Pre-commit Hooks
        # ----------------------------------------------------------------------
        preCommitCheck = git-hooks-nix.lib.${system}.run {
          src = ./.;
          hooks = {
            check-added-large-files.enable = true;
            check-yaml.enable = true;
            end-of-file-fixer.enable = true;
            trim-trailing-whitespace.enable = true;

            nixfmt.enable = true;
            statix.enable = true;
            deadnix.enable = true;

            # Format Rust at commit time. Clippy is run in CI rather than
            # at commit, since it requires the full dev-shell environment
            # (libdav1d) which the bare git hook process doesn't inherit.
            rustfmt = {
              enable = true;
              packageOverrides = {
                cargo = rustToolchain;
                rustfmt = rustToolchain;
              };
            };
          };
        };
      in
      {
        # ----------------------------------------------------------------------
        # Dev Shell
        # ----------------------------------------------------------------------
        devShells.default = pkgs.mkShell {
          name = "kiln-dev";

          packages =
            preCommitCheck.enabledPackages
            ++ [ rustToolchain ]
            ++ (with pkgs; [
              # `image` crate AVIF decoder — libdav1d via dav1d-sys.
              dav1d
              pkg-config
              nasm

              # kiln runtime dependency.
              pagefind
            ])
            # Darwin requires libiconv when crates link against libstd
            # symbols (e.g. onig_sys, libwebp-sys).
            ++ pkgs.lib.optional pkgs.stdenv.isDarwin pkgs.libiconv;

          shellHook =
            preCommitCheck.shellHook
            + pkgs.lib.optionalString pkgs.stdenv.isDarwin ''
              # Point the linker at the system Xcode SDK so rustc's stdlib
              # (which links against system libSystem) resolves cleanly. The
              # Nix apple-sdk sysroot has different ABI versioning.
              export LIBRARY_PATH="$(xcrun --show-sdk-path)/usr/lib''${LIBRARY_PATH:+:$LIBRARY_PATH}"
            '';

          env.RUST_BACKTRACE = "1";
        };

        # ----------------------------------------------------------------------
        # Checks (`nix flake check`)
        # ----------------------------------------------------------------------
        checks = {
          inherit preCommitCheck;
        };

        # ----------------------------------------------------------------------
        # Formatter (`nix fmt`)
        # ----------------------------------------------------------------------
        formatter = pkgs.nixfmt;
      }
    );
}
