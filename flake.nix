# ==============================================================================
# kiln Development Flake
# ==============================================================================
#
# Provides Rust toolchain, libdav1d (AVIF decode), Pagefind, Tailwind CSS, git-cliff,
# and pre-commit hooks. Exposes `packages.{kiln,kiln-tailwindcss,pagefind}` for site
# repos importing this flake.
#
#   nix develop        # interactive shell for hacking on kiln
#   nix flake check    # run pre-commit hooks
#   nix build '.#kiln' # build kiln from source (dav1d wired in by Nix)

{
  description = "kiln — custom static site generator (dev environment)";

  nixConfig = {
    extra-substituters = [ "https://hakula.cachix.org" ];
    extra-trusted-public-keys = [ "hakula.cachix.org-1:7zwB3fhMfReHdOjh6DmnaLXgqbPDBcojvN9F+osZw0k=" ];
  };

  # ----------------------------------------------------------------------------
  # Inputs
  # ----------------------------------------------------------------------------
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";

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

        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [
            "llvm-tools-preview"
            "rust-analyzer"
            "rust-src"
          ];
        };

        cssCompiler = pkgs.callPackage ./packages/css { };

        # Some workspace dependencies require a newer Rust version than nixpkgs provides.
        kiln = pkgs.callPackage ./packages/kiln {
          inherit cssCompiler;
          rustPlatform = pkgs.makeRustPlatform {
            cargo = rustToolchain;
            rustc = rustToolchain;
          };
          src = pkgs.lib.fileset.toSource {
            root = ./.;
            fileset = pkgs.lib.fileset.unions [
              ./Cargo.toml
              ./Cargo.lock
              ./crates
            ];
          };
        };

        # ----------------------------------------------------------------------
        # Node Hook Wrapper
        # ----------------------------------------------------------------------
        # Node hooks need the local dependencies, which the Nix sandbox excludes.
        # CI runs the equivalent checks directly with pnpm.
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

            # Clippy needs libdav1d from the dev shell and runs in CI.
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
              files = "\\.(json|mjs)$";
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

            eslint = {
              enable = true;
              name = "eslint";
              entry = nodeHook "eslint" "eslint --fix";
              files = "\\.(js|mjs)$";
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
            ++ [
              rustToolchain
              cssCompiler
            ]
            ++ (with pkgs; [
              dav1d
              git-cliff
              nasm
              nodejs_24
              pagefind
              pkg-config
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
        # Site repos consume these via `kiln.packages.${system}.<name>`.
        packages = {
          default = kiln;
          inherit kiln;
          inherit (pkgs) pagefind;
          kiln-tailwindcss = cssCompiler;
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
