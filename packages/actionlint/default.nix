{ actionlint, fetchurl }:

actionlint.overrideAttrs (old: {
  patches = (old.patches or [ ]) ++ [
    (fetchurl {
      url = "https://github.com/rhysd/actionlint/commit/644076a59742c2d1540ebd4686eab3c308f0e562.patch";
      hash = "sha256-H2y2MlM35dnlSmt7DFYRKVOzDRYzJ2Tw8hWR5nUnwak=";
    })
  ];
})
