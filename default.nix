{ version, lib, rustPlatform }:

rustPlatform.buildRustPackage {
  pname = "zstdp";
  inherit version;
  src = ./.;
  cargoLock.lockFile = ./Cargo.lock;

  meta = {
    description = "Zstd proxy";
    homepage = "https://github.com/blurgyy/zstdp";
    license = lib.licenses.mit;
  };
}
