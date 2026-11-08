# ==================================================================================================
# kiln Development Flake
# ==================================================================================================

{
  description = "kiln packages and development environment";

  nixConfig = {
    extra-substituters = [ "https://hakula.cachix.org" ];
    extra-trusted-public-keys = [ "hakula.cachix.org-1:7zwB3fhMfReHdOjh6DmnaLXgqbPDBcojvN9F+osZw0k=" ];
  };

  # ------------------------------------------------------------------------------------------------
  # Inputs
  # ------------------------------------------------------------------------------------------------
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";

    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    devkit = {
      url = "github:hakula139/devkit/v1";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    kiln-tailwindcss = {
      url = "github:hakula139/kiln-tailwindcss/v0.1.4";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  # ------------------------------------------------------------------------------------------------
  # Outputs
  # ------------------------------------------------------------------------------------------------
  outputs =
    {
      nixpkgs,
      rust-overlay,
      devkit,
      kiln-tailwindcss,
      ...
    }:
    devkit.lib.eachSystem devkit.lib.systems (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
        };

        rustToolchain = (pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml).override {
          extensions = [
            "llvm-tools-preview"
            "rust-analyzer"
            "rust-src"
          ];
        };

        cssCompiler = kiln-tailwindcss.packages.${system}.default;

        # Some workspace dependencies require a newer Rust version than nixpkgs provides.
        kiln = pkgs.callPackage ./package.nix {
          inherit cssCompiler;
          rustPlatform = pkgs.makeRustPlatform {
            cargo = rustToolchain;
            rustc = rustToolchain;
          };
        };

        # ------------------------------------------------------------------------------------------
        # Pre-commit Hooks
        # ------------------------------------------------------------------------------------------
        dk = devkit.lib.mkLib pkgs;
        preCommit = dk.preCommit {
          src = ./.;
          hooks = {
            actionlint.enable = true;
            check-yaml.enable = true;
            cspell.enable = true;
            deadnix.enable = true;
            dprint.enable = true;
            markdownlint-cli2.enable = true;
            nixfmt.enable = true;
            prettier.enable = true;

            rustfmt = {
              enable = true;
              packageOverrides = {
                cargo = rustToolchain;
                rustfmt = rustToolchain;
              };
            };

            shellcheck.enable = true;
            shfmt.enable = true;
            sort-file-contents.enable = true;
            statix.enable = true;
            taplo.enable = true;
          };
        };

      in
      {
        # ------------------------------------------------------------------------------------------
        # Dev Shell
        # ------------------------------------------------------------------------------------------
        devShells.default = pkgs.mkShell {
          name = "kiln-dev";

          packages = [
            rustToolchain
            cssCompiler
          ]
          ++ (with pkgs; [
            cargo-llvm-cov
            dav1d
            git-cliff
            nodejs_26
            pagefind
            pkg-config
            unzip
          ])
          # libiconv resolves onig_sys / libwebp-sys link errors on darwin.
          ++ pkgs.lib.optional pkgs.stdenv.isDarwin pkgs.libiconv;

          inputsFrom = [ preCommit.shell ];

          env.RUST_BACKTRACE = "1";
        };

        # ------------------------------------------------------------------------------------------
        # Packages
        # ------------------------------------------------------------------------------------------
        packages = {
          default = kiln;
          inherit kiln;
        };

        # ------------------------------------------------------------------------------------------
        # Checks
        # ------------------------------------------------------------------------------------------
        checks.pre-commit = preCommit.check;

        # ------------------------------------------------------------------------------------------
        # Formatter
        # ------------------------------------------------------------------------------------------
        formatter = pkgs.nixfmt-tree;
      }
    );
}
