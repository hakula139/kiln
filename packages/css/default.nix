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
  npmDepsHash = "sha256-eeMTqKO2xK2wCA7eZtIxrR9CdiNk7dhCe8eXEXKZHiI=";
  dontNpmBuild = true;

  meta = {
    description = "Tailwind CSS compiler for kiln";
    homepage = "https://github.com/hakula139/kiln";
    license = lib.licenses.mit;
    mainProgram = "kiln-tailwindcss";
  };
}
