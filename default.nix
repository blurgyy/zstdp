{
  version,
  lib,
  rustPlatform,

  clippy,
  cargo-edit,
  rustfmt,
}:

rustPlatform.buildRustPackage {
  pname = "zstdp";
  inherit version;
  src = ./.;
  cargoLock.lockFile = ./Cargo.lock;

  nativeBuildInputs = [
    clippy
    cargo-edit
    rustfmt
  ];

  shellHook = ''
    SHELL=$(grep "$USER:" /etc/passwd | awk -F: '{ print $NF }')
    [[ $- == *i* ]] && exec $SHELL
  '';

  meta = {
    description = "Zstd proxy";
    homepage = "https://github.com/blurgyy/zstdp";
    license = lib.licenses.asl20;
  };
}
