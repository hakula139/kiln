{
  avifSupport ? true,
  cssCompiler,
  dav1d,
  git,
  lib,
  makeWrapper,
  pagefind,
  pkg-config,
  rustPlatform,
  tzdata,
}:

let
  fs = lib.fileset;
in
rustPlatform.buildRustPackage {
  pname = "kiln";
  inherit ((lib.importTOML ./Cargo.toml).workspace.package) version;

  src = fs.toSource {
    root = ./.;
    fileset = fs.unions [
      ./Cargo.toml
      ./Cargo.lock
      ./crates
    ];
  };
  cargoLock.lockFile = ./Cargo.lock;

  buildFeatures = lib.optional avifSupport "avif";

  nativeBuildInputs = [ makeWrapper ] ++ lib.optional avifSupport pkg-config;
  buildInputs = lib.optional avifSupport dav1d;

  # The dev server tests bind 127.0.0.1, which the darwin sandbox denies by default.
  __darwinAllowLocalNetworking = true;

  # The build sandbox exposes no system zoneinfo, so jiff needs an explicit TZDIR
  # to resolve IANA zone names in the time zone tests.
  nativeCheckInputs = [
    cssCompiler
    git
    pagefind
    tzdata
  ];
  preCheck = ''
    export TZDIR=${tzdata}/share/zoneinfo
  '';

  postFixup = ''
    wrapProgram $out/bin/kiln --prefix PATH : ${
      lib.makeBinPath [
        cssCompiler
        pagefind
      ]
    }
  '';

  meta = {
    description = "Custom static site generator powering hakula.xyz";
    homepage = "https://github.com/hakula139/kiln";
    license = lib.licenses.mit;
    mainProgram = "kiln";
  };
}
