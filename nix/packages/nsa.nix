{
  rustPlatform,
  lib,
}:
rustPlatform.buildRustPackage {
  pname = "nsa";
  version = "0.3.1";
  src = ../../.;

  cargoLock.lockFile = ../../Cargo.lock;

  meta = {
    description = "Discord bot that logs all messages to Postgres";
    homepage = "https://github.com/KNOXDEV/nsa";
    mainProgram = "nsa";
    platforms = lib.platforms.linux;
  };
}
