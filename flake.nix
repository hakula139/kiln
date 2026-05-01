# ==============================================================================
# kiln Development Flake
# ==============================================================================
#
# Provides Rust toolchain, libdav1d (AVIF decode), pagefind, and pre-commit hooks.
#
#   nix develop                            # interactive shell
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

        # Stable Rust with clippy / coverage / editor extensions.
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

            # Clippy stays in CI — the bare hook process can't see libdav1d.
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
              # AVIF decode (dav1d-sys) + kiln's runtime search dep.
              dav1d
              pkg-config
              nasm
              pagefind
            ])
            # libiconv resolves onig_sys / libwebp-sys link errors on darwin.
            ++ pkgs.lib.optional pkgs.stdenv.isDarwin pkgs.libiconv;

          shellHook =
            preCommitCheck.shellHook
            + pkgs.lib.optionalString pkgs.stdenv.isDarwin ''
              # rustc stdlib targets the system libSystem; reach the Xcode SDK
              # rather than the Nix apple-sdk sysroot (mismatched ABI).
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
