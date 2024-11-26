{
  lib,
  stdenv,
  makeRustPlatform,

  buildPackages,
  pkgsStatic,

  pkg-config,
  openssl,
  sqlite,
  systemdMinimal,
  iw,

  # arguments
  buildInputs ? [ ],
  nativeBuildInputs ? [ ],
  fullStatic ? false,
  buildNoDefaultFeatures ? false,
  buildFeatures ? [ ],
}:
let
  inherit (stdenv.hostPlatform) rust;
  toolchain = buildPackages.rust-bin.stable.latest.default.override {
    targets = [ rust.rustcTarget ];
  };
  rustPlatform = makeRustPlatform {
    inherit stdenv;
    cargo = toolchain;
    rustc = toolchain;
  };
in
rustPlatform.buildRustPackage {
  pname = "edgehog-device-runtime";
  version = "0.8.1";

  separateDebugInfo = true;

  src = ../.;

  inherit buildNoDefaultFeatures buildFeatures;

  cargoLock = {
    lockFile = ../Cargo.lock;
  };

  nativeBuildInputs = nativeBuildInputs ++ [
    pkg-config
  ];
  buildInputs =
    buildInputs
    ++ lib.optionals (!buildNoDefaultFeatures) [
      systemdMinimal
      iw
    ]
    ++ (
      if fullStatic then
        [
          pkgsStatic.openssl
          pkgsStatic.sqlite
        ]
      else
        [
          openssl
          sqlite
        ]
    );

  "CARGO_TARGET_${rust.cargoEnvVarTarget}_LINKER" = "${stdenv.cc.targetPrefix}ld";
  CARGO_BUILD_RUSTFLAGS = lib.optional fullStatic "-C target-feature=+crt-static";
}
