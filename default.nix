{
  version,
  lib,
  rustPlatform,

  clippy,
  cargo-edit,
  rustfmt,

  mime-db,

  # jemalloc for the zstdp binary, PREBUILT by nixpkgs for exactly this crate:
  # `tikv-jemalloc-sys`'s own autoconf run fails under the sandbox's hardened
  # cc-wrapper ("cannot determine return type of strerror_r"), and the
  # package's setup hook exports JEMALLOC_OVERRIDE so the crate links this
  # library instead of building one. nixpkgs sets `--with-lg-page` to a fixed
  # per-arch value (64 KiB on aarch64, 4 KiB elsewhere), NOT the builder's or
  # any host's detected page size, so the library runs on every kernel of that
  # arch: jemalloc requires its compiled page size >= the runtime page size, so
  # the arch maximum is the portable choice (fry runs 4 KiB pages under the
  # 64 KiB aarch64 build). The unprefixed
  # variant matches the crate's `unprefixed_malloc_on_supported_platforms`
  # feature (jemalloc becomes the process's malloc, C deps included).
  rust-jemalloc-sys-unprefixed,
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

  # Its setup hook exports JEMALLOC_OVERRIDE (see the argument's note); in
  # buildInputs so the binary's rpath carries the shared library.
  buildInputs = [ rust-jemalloc-sys-unprefixed ];

  MIME_DB_PATH = mime-db;

  shellHook = ''
    SHELL=$(grep "$USER:" /etc/passwd | awk -F: '{ print $NF }')
    [[ $- == *i* ]] && exec $SHELL
  '';

  meta = {
    description = "Zstd proxy";
    homepage = "https://github.com/blurgyy/zstdp";
    mainProgram = "zstdp";
    license = lib.licenses.asl20;
  };
}
