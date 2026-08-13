{
  config,
  pkgs,
  lib,
  ...
}: let
  cfg = config.services.nsa;
in {
  options.services.nsa = {
    enable = lib.mkEnableOption "nsa discord bot";

    environmentFile = lib.mkOption {
      type = lib.types.path;
      description = ''
        Path to a root-readable file containing newline-separated
        DISCORD_TOKEN=... and PG_PASSWORD=... entries. Typically the
        decrypted path of an agenix secret.
      '';
    };

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.internal.nsa;
      description = "The nsa package to run.";
    };

    attachmentsMaxBytes = lib.mkOption {
      type = lib.types.nullOr lib.types.int;
      default = null; # bot default: 100 MiB
      description = ''
        Cap for locally stored attachment files; oversized ones are recorded
        but skipped, and re-fetched automatically if the cap is later raised.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    users.users.nsa = {
      isSystemUser = true;
      group = "nsa";
    };
    users.groups.nsa = {};

    systemd.services.nsa-postgres-password = {
      description = "Sync nsa postgres role password from environmentFile";
      after = ["postgresql.service"];
      requires = ["postgresql.service"];
      before = ["nsa.service"];
      wantedBy = ["multi-user.target"];
      serviceConfig = {
        Type = "oneshot";
        RemainAfterExit = true;
      };
      script = ''
        set -euo pipefail
        PG_PASSWORD=$(${pkgs.gawk}/bin/awk -F= '/^PG_PASSWORD=/ {
          print substr($0, index($0, "=") + 1)
        }' ${cfg.environmentFile})
        # psql's :'pw' interpolation requires script-mode input — `-c` sends the
        # command unparsed to the server, so we pipe via stdin instead.
        ${pkgs.sudo}/bin/sudo -u postgres ${config.services.postgresql.package}/bin/psql \
          -v ON_ERROR_STOP=1 \
          -v "pw=$PG_PASSWORD" \
          <<<"ALTER USER nsa WITH PASSWORD :'pw';"
      '';
    };

    systemd.services.nsa = {
      description = "nsa discord logger";
      wantedBy = ["multi-user.target"];
      after = [
        "network-online.target"
        "postgresql.service"
        "nsa-postgres-password.service"
      ];
      wants = ["network-online.target"];
      requires = [
        "postgresql.service"
        "nsa-postgres-password.service"
      ];
      environment =
        {
          PG_HOST = "/run/postgresql";
          PG_USER = "nsa";
          ATTACHMENTS_DIR = "/var/lib/nsa/attachments";
        }
        // lib.optionalAttrs (cfg.attachmentsMaxBytes != null) {
          ATTACHMENTS_MAX_BYTES = toString cfg.attachmentsMaxBytes;
        };
      serviceConfig = {
        ExecStart = lib.getExe cfg.package;
        EnvironmentFile = cfg.environmentFile;
        User = "nsa";
        Group = "nsa";
        Restart = "always";
        RestartSec = "5s";
        # writable exemption from ProtectSystem=strict for the attachment file store;
        # 0700 because captured Discord content is sensitive
        StateDirectory = "nsa";
        StateDirectoryMode = "0700";
        NoNewPrivileges = true;
        PrivateTmp = true;
        ProtectSystem = "strict";
        ProtectHome = true;
        ProtectKernelTunables = true;
        ProtectKernelModules = true;
        ProtectControlGroups = true;
        RestrictAddressFamilies = ["AF_UNIX" "AF_INET" "AF_INET6"];
      };
    };
  };
}
