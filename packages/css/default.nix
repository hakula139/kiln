{
  buildNpmPackage,
  lib,
}:

let
  package = lib.importJSON ./package.json;
in
buildNpmPackage {
  pname = "kiln-tailwindcss";
  inherit (package) version;

  src = lib.fileset.toSource {
    root = ./.;
    fileset = lib.fileset.unions [
      ./compiler.mjs
      ./package.json
      ./package-lock.json
    ];
  };
  npmDepsHash = "sha256-hX/BVs/LT5AWvTK+kt1aCLUptgkPZEM6all6QhQxblg=";
  dontNpmBuild = true;

  meta = {
    description = "Tailwind CSS compiler for kiln";
    homepage = "https://github.com/hakula139/kiln";
    license = lib.licenses.mit;
    mainProgram = "kiln-tailwindcss";
  };
}
