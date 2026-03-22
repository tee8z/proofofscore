{ config, pkgs, lib, ... }:

let
  appDir = "/opt/proofofscore";
in
{
  imports = [ ./hardware-configuration.nix ];

  # ============================================
  # System basics
  # ============================================

  system.stateVersion = "24.11";

  nix.settings.experimental-features = [ "nix-command" "flakes" ];

  # Allow running dynamically linked binaries built on standard Linux (e.g. Ubuntu CI)
  programs.nix-ld.enable = true;

  time.timeZone = "UTC";

  # ============================================
  # Networking & Firewall
  # ============================================

  networking.hostName = "proofofscore";

  networking.firewall = {
    enable = true;
    allowedTCPPorts = [
      22   # SSH
      80   # HTTP (Caddy redirect)
      443  # HTTPS (Caddy)
    ];
    allowedUDPPorts = [
      51820 # WireGuard
    ];
  };

  # ============================================
  # WireGuard VPN (for admin dashboard access)
  # ============================================

  networking.wg-quick.interfaces.wg0 = {
    address = [ "10.100.0.1/24" ];
    listenPort = 51820;
    privateKeyFile = "/opt/proofofscore/secrets/wg-private-key";

    peers = [
      {
        publicKey = builtins.readFile /opt/proofofscore/secrets/wg-client-public-key;
        allowedIPs = [ "10.100.0.2/32" ];
      }
    ];
  };

  # ============================================
  # Users
  # ============================================

  users.users.deploy = {
    isNormalUser = true;
    extraGroups = [ "wheel" ];
    openssh.authorizedKeys.keyFiles = [
      /opt/proofofscore/secrets/authorized_keys
    ];
  };

  users.users.proofofscore = {
    isSystemUser = true;
    group = "proofofscore";
    home = appDir;
    createHome = true;
  };

  users.groups.proofofscore = {};

  # ============================================
  # SSH
  # ============================================

  users.users.root.openssh.authorizedKeys.keyFiles = [
    /opt/proofofscore/secrets/authorized_keys
  ];

  services.openssh = {
    enable = true;
    settings = {
      PermitRootLogin = "prohibit-password";
      PasswordAuthentication = false;
    };
  };

  # ============================================
  # Caddy (reverse proxy + auto TLS)
  # ============================================

  services.caddy = {
    enable = true;
    # Caddy auto-provisions Let's Encrypt certs
    virtualHosts."proofofscore.win" = {
      extraConfig = ''
        @hashed_assets {
          path_regexp \.(css|js)$
          path /static/*
        }
        header @hashed_assets Cache-Control "public, max-age=31536000, immutable"

        @other_static {
          not path_regexp \.(css|js)$
          path /static/*
        }
        header @other_static Cache-Control "public, max-age=3600"

        reverse_proxy 127.0.0.1:8900
      '';
    };
  };

  # ============================================
  # Proof of Score service
  # ============================================

  systemd.services.proofofscore = {
    description = "Proof of Score Game Server";
    after = [ "network.target" ];
    wantedBy = [ "multi-user.target" ];

    serviceConfig = {
      Type = "simple";
      User = "proofofscore";
      Group = "proofofscore";
      WorkingDirectory = appDir;
      ExecStart = "${appDir}/bin/server -c ${appDir}/config/production.toml";
      Restart = "on-failure";
      RestartSec = 5;

      # Security hardening
      NoNewPrivileges = true;
      ProtectSystem = "strict";
      ProtectHome = true;
      ReadWritePaths = [ "${appDir}/data" "${appDir}/backups" "${appDir}/creds" ];
      ReadOnlyPaths = [ "${appDir}/bin" "${appDir}/config" "${appDir}/ui" "${appDir}/static" "${appDir}/secrets" "${appDir}/migrations" ];
    };

    preStart = ''
      # Ensure directories exist
      mkdir -p ${appDir}/{data,backups,creds}
    '';
  };

  # ============================================
  # Database backup to Backblaze B2
  # ============================================

  systemd.services.proofofscore-backup = {
    description = "Backup Proof of Score SQLite to Backblaze B2";
    serviceConfig = {
      Type = "oneshot";
      User = "proofofscore";
      Group = "proofofscore";
    };
    path = [ pkgs.sqlite pkgs.backblaze-b2 pkgs.coreutils pkgs.bash ];
    script = let
      backupScript = pkgs.writeShellScript "proofofscore-backup" ''
        set -euo pipefail

        DB_PATH="${appDir}/data/game.db"
        BACKUP_DIR="${appDir}/backups"
        DATE=$(date +%Y%m%d-%H%M%S)
        BACKUP_FILE="$BACKUP_DIR/game-$DATE.db"
        B2_BUCKET="proofofscore-backup-prod"

        if [ ! -f "$DB_PATH" ]; then
          echo "No database to backup"
          exit 0
        fi

        sqlite3 "$DB_PATH" ".backup '$BACKUP_FILE'"
        echo "Local backup: $BACKUP_FILE"

        CREDS="${appDir}/secrets/b2_credentials"
        if [ -f "$CREDS" ]; then
          source "$CREDS"
          b2 authorize-account "$B2_KEY_ID" "$B2_APP_KEY" 2>/dev/null
          b2 upload-file "$B2_BUCKET" "$BACKUP_FILE" "backups/game-$DATE.db"
          echo "Uploaded to B2"
        fi

        ls -t "$BACKUP_DIR"/game-*.db 2>/dev/null | tail -n +8 | xargs -r rm
      '';
    in "${backupScript}";
  };

  systemd.timers.proofofscore-backup = {
    description = "Daily backup of Proof of Score database";
    wantedBy = [ "timers.target" ];
    timerConfig = {
      OnCalendar = "*-*-* 03:00:00 UTC";
      Persistent = true;
    };
  };

  # ============================================
  # System packages
  # ============================================

  environment.systemPackages = with pkgs; [
    vim
    htop
    sqlite
    backblaze-b2
    rsync
  ];

  # ============================================
  # Automatic security updates
  # ============================================

  system.autoUpgrade = {
    enable = true;
    allowReboot = false; # Don't auto-reboot, just upgrade packages
  };
}
