# kiln — built from source. dav1d is a real buildInputs entry; Nix wires the
# correct rpath / install_name on Linux and Darwin, no post-build patching.

{
  cargoLock,
  cssCompiler,
  dav1d,
  git,
  lib,
  makeWrapper,
  nasm,
  pkg-config,
  rustPlatform,
  src,
  tzdata,
  version,
}:

rustPlatform.buildRustPackage {
  pname = "kiln";
  inherit cargoLock src version;

  nativeBuildInputs = [
    makeWrapper
    nasm
    pkg-config
  ];
  buildInputs = [ dav1d ];

  # The dev server tests bind 127.0.0.1, which the darwin sandbox denies by default.
  __darwinAllowLocalNetworking = true;

  # The build sandbox exposes no system zoneinfo, so jiff needs an explicit TZDIR
  # to resolve IANA zone names in the time zone tests.
  nativeCheckInputs = [
    cssCompiler
    git
    tzdata
  ];
  preCheck = ''
    export TZDIR=${tzdata}/share/zoneinfo
  '';

  postFixup = ''
    wrapProgram $out/bin/kiln --prefix PATH : ${lib.makeBinPath [ cssCompiler ]}
  '';

  meta = {
    description = "Custom static site generator powering hakula.xyz";
    homepage = "https://github.com/hakula139/kiln";
    license = lib.licenses.mit;
    mainProgram = "kiln";
  };
}
