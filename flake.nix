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
      url = "github:hakula139/devkit/v3";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    kiln-tailwindcss = {
      url = "github:hakula139/kiln-tailwindcss/v0.1.6";
      inputs = {
        devkit.follows = "devkit";
        nixpkgs.follows = "nixpkgs";
      };
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

        rustVersion = (pkgs.lib.importTOML ./Cargo.toml).workspace.package.rust-version;
        rustBin = rust-overlay.lib.mkRustBin { } pkgs;
        rustToolchain = rustBin.fromRustupToolchainFile ./rust-toolchain.toml;

        cssCompiler = kiln-tailwindcss.packages.${system}.default;

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
            clippy = {
              enable = true;
              packageOverrides = {
                cargo = rustToolchain;
                clippy = rustToolchain;
              };
              settings.extraArgs = "--locked --all-targets --all-features";
            };

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
            taplo.enable = true;
          };
        };

      in
      {
        # ------------------------------------------------------------------------------------------
        # Dev Shells
        # ------------------------------------------------------------------------------------------
        devShells = {
          default = pkgs.mkShell {
            name = "kiln-dev";

            packages = [
              rustToolchain
              cssCompiler
              dk.nodeToolchain.nodejs
            ]
            ++ (with pkgs; [
              cargo-llvm-cov
              dav1d
              git-cliff
              pagefind
              pkg-config
              unzip
            ])
            ++ pkgs.lib.optional pkgs.stdenv.isDarwin pkgs.libiconv;

            inputsFrom = [ preCommit ];

            env.RUST_BACKTRACE = "1";
          };

          msrv = pkgs.mkShell {
            name = "kiln-msrv";

            packages = [
              rustBin.stable.${pkgs.lib.versions.pad 3 rustVersion}.minimal
            ]
            ++ (with pkgs; [
              dav1d
              pkg-config
            ]);
          };
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
        checks.package = kiln;

        # ------------------------------------------------------------------------------------------
        # Formatter
        # ------------------------------------------------------------------------------------------
        formatter = pkgs.nixfmt-tree;
      }
    );
}
