{
  lib,
  avifSupport ? true,
  cssCompiler,
  dav1d,
  git,
  makeWrapper,
  pagefind,
  pkg-config,
  rustPlatform,
  tzdata,
}:

rustPlatform.buildRustPackage {
  inherit ((lib.importTOML ./Cargo.toml).workspace.package) version;
  pname = "kiln";

  src = lib.fileset.toSource {
    root = ./.;
    fileset = lib.fileset.unions [
      ./Cargo.toml
      ./Cargo.lock
      ./crates
    ];
  };
  cargoLock.lockFile = ./Cargo.lock;

  buildFeatures = lib.optional avifSupport "avif";

  nativeBuildInputs = [ makeWrapper ] ++ lib.optional avifSupport pkg-config;
  buildInputs = lib.optional avifSupport dav1d;

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
