# kiln — built from source. dav1d is a real buildInputs entry; Nix wires the
# correct rpath / install_name on Linux and Darwin, no post-build patching.

{
  lib,
  rustPlatform,
  pkg-config,
  nasm,
  dav1d,
  tzdata,
}:

let
  cargoToml = fromTOML (builtins.readFile ../../Cargo.toml);

  excluded = [
    ".claude"
    ".github"
    "docs"
    "packages"
    "CHANGELOG.md"
    "CLAUDE.md"
    "README.md"
    "RELEASING.md"
    "cliff.toml"
    "codecov.yml"
    "cspell.json"
  ];

  src = lib.cleanSourceWith {
    src = ../..;
    filter =
      path: _type:
      let
        rel = lib.removePrefix (toString ../.. + "/") (toString path);
        firstSegment = lib.head (lib.splitString "/" rel);
      in
      !(lib.elem firstSegment excluded);
  };
in
rustPlatform.buildRustPackage {
  pname = "kiln";
  inherit (cargoToml.workspace.package) version;

  inherit src;
  cargoLock.lockFile = ../../Cargo.lock;

  nativeBuildInputs = [
    pkg-config
    nasm
  ];
  buildInputs = [ dav1d ];

  # The build sandbox exposes no system zoneinfo, so jiff needs an explicit TZDIR
  # to resolve IANA zone names in the time zone tests.
  nativeCheckInputs = [ tzdata ];
  preCheck = ''
    export TZDIR=${tzdata}/share/zoneinfo
  '';

  meta = {
    description = "Custom static site generator powering hakula.xyz";
    homepage = "https://github.com/hakula139/kiln";
    license = lib.licenses.mit;
    mainProgram = "kiln";
  };
}
