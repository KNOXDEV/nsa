{
  rustPlatform,
  lib,
}:
rustPlatform.buildRustPackage {
  pname = "nsa";
  version = "0.3.2";
  src = ../../.;

  cargoHash = "sha256-l/+q6+28+0Jd6Ns8kzio07vZwC3N0pT+flXy5N/sS9s=";

  meta = {
    description = "Discord bot that logs all messages to Postgres";
    homepage = "https://github.com/KNOXDEV/nsa";
    mainProgram = "nsa";
    platforms = lib.platforms.linux;
  };
}
