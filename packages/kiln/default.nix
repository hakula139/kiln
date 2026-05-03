# kiln — built from source. dav1d is a real buildInputs entry; Nix wires the
# correct rpath / install_name on Linux and Darwin, no post-build patching.

{
  lib,
  rustPlatform,
  pkg-config,
  nasm,
  dav1d,
}:

let
  cargoToml = builtins.fromTOML (builtins.readFile ../../Cargo.toml);
in
rustPlatform.buildRustPackage {
  pname = "kiln";
  inherit (cargoToml.workspace.package) version;

  src = lib.cleanSource ../..;
  cargoLock.lockFile = ../../Cargo.lock;

  nativeBuildInputs = [
    pkg-config
    nasm
  ];
  buildInputs = [ dav1d ];

  meta = {
    description = "Custom static site generator powering hakula.xyz";
    homepage = "https://github.com/hakula139/kiln";
    license = lib.licenses.mit;
    mainProgram = "kiln";
  };
}
