{
  source,
  stdenvNoCC,
  jq,
}:

stdenvNoCC.mkDerivation {
  inherit (source) pname version src;

  nativeBuildInputs = [
    jq
  ];

  buildCommand = ''
    jq . -c $src/db.json >$out
  '';
}
