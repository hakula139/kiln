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
    # Nixpkgs - NixOS 25.11 stable release
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.11";

    # Per-system flake outputs
    flake-utils.url = "github:numtide/flake-utils";

    # Rust toolchains
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    # Pre-commit hooks
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
        # Node Hook Wrapper
        # ----------------------------------------------------------------------
        # `pnpm exec` needs node + pnpm on PATH and the project's
        # `node_modules` materialised. The Nix sandbox lacks the latter, so
        # `nix flake check` skips these hooks; the equivalent checks run in
        # CI via direct `pnpm` scripts.
        nodeHook =
          name: cmd:
          let
            wrapper = pkgs.writeShellApplication {
              inherit name;
              runtimeInputs = [
                pkgs.nodejs_24
                pkgs.pnpm
              ];
              text = ''
                if [ ! -d node_modules ]; then
                  exit 0
                fi
                pnpm exec ${cmd} "$@"
              '';
            };
          in
          "${wrapper}/bin/${name}";

        # ----------------------------------------------------------------------
        # Pre-commit Hooks
        # ----------------------------------------------------------------------
        preCommitCheck = git-hooks-nix.lib.${system}.run {
          src = ./.;
          hooks = {
            check-added-large-files.enable = true;
            check-yaml.enable = true;
            end-of-file-fixer.enable = true;
            # Preserve Markdown's two-trailing-space hard-break syntax.
            trim-trailing-whitespace = {
              enable = true;
              args = [ "--markdown-linebreak-ext=md" ];
            };

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

            prettier-write = {
              enable = true;
              name = "prettier";
              entry = nodeHook "prettier-write" "prettier --write --ignore-unknown";
              # Markdown is opinionated; markdownlint covers structure. JSON
              # is safe to auto-format.
              files = "\\.json$";
              pass_filenames = true;
            };

            markdownlint = {
              enable = true;
              name = "markdownlint-cli2";
              entry = nodeHook "markdownlint" "markdownlint-cli2";
              files = "\\.md$";
              pass_filenames = true;
            };

            cspell = {
              enable = true;
              entry = nodeHook "cspell" "cspell --no-must-find-files --no-progress";
              types = [ "text" ];
              pass_filenames = true;
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
              # Node tooling for the Node-side pre-commit hooks.
              nodejs_24
              pnpm
            ])
            # libiconv resolves onig_sys / libwebp-sys link errors on darwin.
            ++ pkgs.lib.optional pkgs.stdenv.isDarwin pkgs.libiconv;

          shellHook =
            preCommitCheck.shellHook
            + pkgs.lib.optionalString pkgs.stdenv.isDarwin ''
              # rustc stdlib targets the system libSystem; reach the Xcode SDK
              # rather than the Nix apple-sdk sysroot (mismatched ABI). Guard
              # `xcrun` so a Darwin user missing the Command Line Tools sees a
              # readable error instead of `LIBRARY_PATH=/usr/lib`.
              if command -v xcrun >/dev/null 2>&1; then
                export LIBRARY_PATH="$(xcrun --show-sdk-path)/usr/lib''${LIBRARY_PATH:+:$LIBRARY_PATH}"
              else
                echo "warning: xcrun not found — run \`xcode-select --install\` so cargo can link against the system SDK" >&2
              fi
            '';

          env.RUST_BACKTRACE = "1";
        };

        # ----------------------------------------------------------------------
        # Checks (`nix flake check`)
        # ----------------------------------------------------------------------
        checks = {
          pre-commit = preCommitCheck;
        };

        # ----------------------------------------------------------------------
        # Formatter (`nix fmt`)
        # ----------------------------------------------------------------------
        formatter = pkgs.nixfmt;
      }
    );
}
