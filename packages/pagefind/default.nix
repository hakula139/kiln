# Pagefind — prebuilt release binary (extended CJK build).
# Nixpkgs ships 1.4.0; we need 1.5+ for the component-UI bundle.

{
  pkgs,
  lib,
  ...
}:

let
  inherit (pkgs.stdenv.hostPlatform) isLinux;
  version = "1.5.2";
  baseUrl = "https://github.com/Pagefind/pagefind/releases/download/v${version}";

  sources = {
    aarch64-darwin = {
      url = "${baseUrl}/pagefind_extended-v${version}-aarch64-apple-darwin.tar.gz";
      hash = "sha256-mcS4gsgcPA8EbvhdGI2uuPb0tZg0TohcynIkx2cPrsQ=";
    };
    x86_64-linux = {
      url = "${baseUrl}/pagefind_extended-v${version}-x86_64-unknown-linux-musl.tar.gz";
      hash = "sha256-rrE1knhW56SYFs8snC5TFn+Hr6H2H3KIt9ZsI0qVDTg=";
    };
  };

  platform = pkgs.stdenv.hostPlatform.system;
  source = sources.${platform} or (throw "Unsupported platform: ${platform}");
in
pkgs.stdenv.mkDerivation {
  pname = "pagefind";
  inherit version;

  src = pkgs.fetchurl {
    inherit (source) url hash;
  };

  sourceRoot = ".";

  nativeBuildInputs = lib.optionals isLinux [ pkgs.autoPatchelfHook ];
  buildInputs = lib.optionals isLinux [ pkgs.stdenv.cc.cc.lib ];

  installPhase = ''
    runHook preInstall
    install -D -m 0755 pagefind_extended $out/bin/pagefind
    runHook postInstall
  '';

  meta = {
    description = "Static-site search index + UI (extended CJK build)";
    homepage = "https://pagefind.app/";
    license = lib.licenses.mit;
    platforms = builtins.attrNames sources;
    mainProgram = "pagefind";
  };
}
