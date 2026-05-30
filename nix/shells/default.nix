{pkgs, ...}:
pkgs.mkShell {
  packages = [
    pkgs.cargo
    pkgs.rustc
    pkgs.rustfmt
    pkgs.clippy
    pkgs.pkg-config
    pkgs.postgresql
    pkgs.process-compose
  ];
  env = {
    RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
  };
  shellHook = ''
    export PGDATA="$PWD/.pgdata"
    export PG_HOST="$PGDATA" # socket dir, libpq-style — matches module.nix
    export PG_USER=nsa
    export PG_PASSWORD=nsa # ignored under trust auth, but the bot requires it set
    export PGDATABASE=nsa

    echo "nsa devshell — run 'process-compose' to start postgres, then 'cargo run'"
  '';
}
