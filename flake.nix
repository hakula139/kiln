# ==============================================================================
# kiln Development Flake
# ==============================================================================
#
# Provides Rust toolchain, libdav1d (AVIF decode), pagefind, git-cliff, and
# pre-commit hooks. Also exposes `packages.{kiln,pagefind}` for downstream
# consumers (site repos importing this flake).
#
#   nix develop        # interactive shell for hacking on kiln
#   nix flake check    # run pre-commit hooks
#   nix build '.#kiln' # build kiln from source (dav1d wired in by Nix)

{
  description = "kiln — custom static site generator (dev environment)";

  # ----------------------------------------------------------------------------
  # Inputs
  # ----------------------------------------------------------------------------
  inputs = {
    # Nixpkgs - NixOS 26.05 stable release
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";

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
        overlays = [
          rust-overlay.overlays.default
          # `pagefind` is a vendored prebuilt; expose it as `pkgs.pagefind`.
          # `kiln` is built from source and stays out of the overlay so it can
          # depend on `rustToolchain` without a `pkgs`-evaluation cycle.
          (final: _: {
            pagefind = final.callPackage ./packages/pagefind { };
          })
        ];

        pkgs = import nixpkgs { inherit system overlays; };

        # Stable Rust with clippy / coverage / editor extensions.
        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [
            "llvm-tools-preview"
            "rust-analyzer"
            "rust-src"
          ];
        };

        # Source-build kiln with the rust-overlay toolchain — nixpkgs's stable
        # rustc lags behind some workspace deps' minimum required version.
        kiln = pkgs.callPackage ./packages/kiln {
          rustPlatform = pkgs.makeRustPlatform {
            cargo = rustToolchain;
            rustc = rustToolchain;
          };
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
              files = "\\.json$";
              pass_filenames = true;
            };

            dprint-write = {
              enable = true;
              name = "dprint";
              entry = nodeHook "dprint-write" "dprint fmt";
              files = "\\.md$";
              pass_filenames = true;
            };

            taplo-write = {
              enable = true;
              name = "taplo";
              entry = nodeHook "taplo-write" "taplo format";
              files = "\\.toml$";
              pass_filenames = true;
            };

            markdownlint = {
              enable = true;
              name = "markdownlint-cli2";
              entry = nodeHook "markdownlint" "markdownlint-cli2 --fix";
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
              # Native build deps (AVIF decode via dav1d-sys).
              dav1d
              nasm
              pkg-config
              # Release tooling.
              git-cliff
              # Search backend invoked by `kiln build` when `[search] enabled`.
              pagefind
              # Node tooling for pre-commit hooks.
              nodejs_24
              pnpm
            ])
            # libiconv resolves onig_sys / libwebp-sys link errors on darwin.
            ++ pkgs.lib.optional pkgs.stdenv.isDarwin pkgs.libiconv;

          shellHook =
            preCommitCheck.shellHook
            + pkgs.lib.optionalString pkgs.stdenv.isDarwin ''
              # Point LIBRARY_PATH at the Xcode SDK so rustc can link on Darwin.
              if command -v xcrun >/dev/null 2>&1; then
                export LIBRARY_PATH="$(xcrun --show-sdk-path)/usr/lib''${LIBRARY_PATH:+:$LIBRARY_PATH}"
              else
                echo "warning: xcrun not found — run \`xcode-select --install\` so cargo can link against the system SDK" >&2
              fi
            '';

          env.RUST_BACKTRACE = "1";
        };

        # ----------------------------------------------------------------------
        # Packages (`nix build '.#<name>'`)
        # ----------------------------------------------------------------------
        # `kiln` is source-built; `pagefind` is a vendored prebuilt. Site repos
        # importing this flake get both via `kiln.packages.${system}.<name>`.
        packages = {
          default = kiln;
          inherit kiln;
          inherit (pkgs) pagefind;
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
        formatter = pkgs.nixfmt-tree;
      }
    );
}
