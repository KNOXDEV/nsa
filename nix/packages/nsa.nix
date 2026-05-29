{
  rustPlatform,
  lib,
}:
rustPlatform.buildRustPackage {
  pname = "nsa";
  version = "0.3.2";
  src = ../../.;

  cargoHash = "sha256-WjiTpDUlXy9yiMjk059pDVW2iFEQSEz1O9aOvk1LpkU=";

  meta = {
    description = "Discord bot that logs all messages to Postgres";
    homepage = "https://github.com/KNOXDEV/nsa";
    mainProgram = "nsa";
    platforms = lib.platforms.linux;
  };
}
