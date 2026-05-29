{
  rustPlatform,
  lib,
}:
rustPlatform.buildRustPackage {
  pname = "nsa";
  version = "0.3.1";
  src = ../../.;

  cargoHash = "sha256-sgE68LZ7SZqdB1j5lEpD7P6UlAcgZV2iYHmErwNgUZY=";

  meta = {
    description = "Discord bot that logs all messages to Postgres";
    homepage = "https://github.com/KNOXDEV/nsa";
    mainProgram = "nsa";
    platforms = lib.platforms.linux;
  };
}
