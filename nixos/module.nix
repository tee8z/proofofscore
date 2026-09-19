{ config, lib, pkgs, ... }:

# A self-contained systemd-nspawn deployment. The public proxy blocks /admin;
# an optional operator hostname admits only explicitly configured sources.
let
  cfg = config.services.proofofscore;
  inherit (cfg) stateDir hostAddress containerAddress port account;
  domain = if cfg.domain == null then "proofofscore.invalid" else cfg.domain;
  server = cfg.package;
  containerState = "/var/lib/proofofscore";
  operator = cfg.operator;
  operatorEnabled = operator.domain != null;
  configuration = pkgs.writeText "proofofscore.toml" ''
    [db_settings]
    data_folder = "${containerState}/data"
    migrations_folder = "${server}/share/proofofscore/migrations"

    [api_settings]
    domain = "0.0.0.0"
    port = "${toString port}"
    private_key_file = "${containerState}/creds/private.pem"
    voltage_api_key = ""
    voltage_api_url = ""
    voltage_org_id = ""
    voltage_env_id = ""
    voltage_wallet_id = ""

    [ui_settings]
    remote_url = "https://${domain}"
    ui_dir = "${containerState}/ui"
    static_dir = "${server}/share/proofofscore/static"

    [ln_settings]
    provider = "lnd"
    lnd_base_url = "${cfg.lndUrl}"
    lnd_macaroon_path = "${containerState}/secrets/admin.macaroon"

    [competition_settings]
    start_time = "00:00"
    duration_secs = 86400
    entry_fee_sats = 1000
    plays_per_payment = 5
    plays_ttl_minutes = 60
    prize_pool_pct = 80

    [bot_detection]
    enabled = true
    max_accounts_per_ip_per_hour = 5
    max_sessions_per_ip_per_hour = 20
    min_timing_variance_us2 = 1000
    max_mean_offset_us = 50000

    [admin]
    # Requests arrive from the host proxy after its operator source check.
    allowed_subnets = ["${hostAddress}/32", "127.0.0.1/32"]
  '';
in
{
  options.services.proofofscore = with lib; {
    enable = mkEnableOption "Proof of Score in an isolated container";
    package = mkOption {
      type = types.nullOr types.package;
      default = null;
      description = "Server package with bin/server and share/proofofscore/{static,migrations}. The flake module supplies packages.server.";
    };
    domain = mkOption {
      type = types.nullOr types.str;
      default = null;
      example = "scores.example.org";
      description = "Public DNS hostname; required when enabled.";
    };
    stateDir = mkOption {
      type = types.str;
      default = "/var/lib/proofofscore";
      description = "Persistent host directory mounted at /var/lib/proofofscore inside the container.";
    };
    hostAddress = mkOption { type = types.str; default = "10.91.0.1"; description = "Host-side IPv4 address of the private container link."; };
    containerAddress = mkOption { type = types.str; default = "10.91.0.2"; description = "Container-side IPv4 address of the private link."; };
    port = mkOption { type = types.port; default = 8900; description = "HTTP listener inside the container."; };
    account = mkOption { type = types.ints.between 1 65534; default = 61200; description = "Numeric UID and GID for persistent application state."; };
    lndUrl = mkOption {
      type = types.str;
      default = "";
      example = "https://lnd.example.org:8080";
      description = "Explicit LND REST endpoint. Supply its macaroon at stateDir/secrets/admin.macaroon.";
    };
    operator = {
      domain = mkOption { type = types.nullOr types.str; default = null; description = "Optional hostname for operator routes."; };
      allowedCIDRs = mkOption { type = types.listOf types.str; default = [ ]; description = "Source CIDRs permitted on the operator hostname."; };
      acmeHost = mkOption { type = types.nullOr types.str; default = null; description = "Existing ACME certificate name; null uses Caddy's internal certificate authority."; };
    };
    manageGateway = mkOption {
      type = types.bool;
      default = true;
      description = "Enable the host Caddy gateway and outbound container NAT. False lets a shared host gateway own those settings.";
    };
    gateway = {
      httpPort = mkOption { type = types.port; default = 80; description = "Host HTTP listener when manageGateway is enabled."; };
      httpsPort = mkOption { type = types.port; default = 443; description = "Host HTTPS listener when manageGateway is enabled."; };
      openFirewall = mkOption { type = types.bool; default = false; description = "Open gateway ports in the host firewall when manageGateway is enabled."; };
    };
    storage = {
      prepareService = mkOption { type = types.str; default = "proofofscore-storage"; description = "Host preparation service basename, without .service."; };
      managePrepareService = mkOption { type = types.bool; default = true; description = "Define the preparation service; false only appends directory creation to an existing service."; };
      mountUnit = mkOption { type = types.nullOr types.str; default = null; example = "srv-apps.mount"; description = "Optional mount unit whose loss stops the container."; };
      slice = mkOption { type = types.str; default = "system.slice"; description = "Host systemd slice for the container."; };
      exportDir = mkOption { type = types.str; default = "/var/lib/proofofscore-backups"; description = "Host directory for the consistent nightly SQLite export."; };
      exportService = mkOption { type = types.str; default = "proofofscore-export"; description = "SQLite export service and timer basename."; };
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = cfg.domain != null && builtins.match "[a-z0-9]([a-z0-9-]*[a-z0-9])?(\\.[a-z0-9]([a-z0-9-]*[a-z0-9])?)+" domain != null;
        message = "services.proofofscore.domain must be a lowercase DNS hostname.";
      }
      { assertion = server != null; message = "Set services.proofofscore.package or import the application's flake module."; }
      { assertion = lib.hasPrefix "https://" cfg.lndUrl; message = "Set services.proofofscore.lndUrl to the explicit HTTPS LND REST endpoint."; }
      { assertion = !operatorEnabled || operator.allowedCIDRs != [ ]; message = "An operator hostname requires explicit services.proofofscore.operator.allowedCIDRs."; }
      { assertion = !operatorEnabled || operator.domain != domain; message = "Proof of Score public and operator hostnames must differ."; }
      { assertion = lib.hasPrefix "/" stateDir && lib.hasPrefix "/" cfg.storage.exportDir; message = "Proof of Score state and export directories must be absolute paths."; }
    ];

    systemd.services.${cfg.storage.prepareService} = lib.mkMerge [
      (lib.mkIf cfg.storage.managePrepareService {
        description = "Prepare Proof of Score persistent state";
        unitConfig.RequiresMountsFor = stateDir;
        serviceConfig = { Type = "oneshot"; RemainAfterExit = true; };
      })
      {
        script = lib.mkAfter ''
          ${pkgs.coreutils}/bin/install -d -m 0750 -o ${toString account} -g ${toString account} \
            ${lib.escapeShellArgs [ stateDir "${stateDir}/data" "${stateDir}/creds" "${stateDir}/secrets" "${stateDir}/ui" ]}
        '';
      }
    ];

    containers.proofofscore = {
      autoStart = true;
      privateNetwork = true;
      inherit hostAddress;
      localAddress = containerAddress;
      bindMounts.${containerState} = { hostPath = stateDir; isReadOnly = false; };
      config = { ... }: {
        system.stateVersion = "26.05";
        networking.firewall.allowedTCPPorts = [ port ];
        users.groups.proofofscore.gid = account;
        users.users.proofofscore = {
          isSystemUser = true;
          uid = account;
          group = "proofofscore";
          home = containerState;
        };
        systemd.services.proofofscore = {
          description = "Proof of Score game server";
          wantedBy = [ "multi-user.target" ];
          wants = [ "network-online.target" ];
          after = [ "network-online.target" ];
          preStart = ''
            umask 077
            mkdir -p ${containerState}/data ${containerState}/creds ${containerState}/secrets ${containerState}/ui
            if [ ! -s ${containerState}/secrets/admin.macaroon ]; then
              echo "No LND macaroon at ${containerState}/secrets/admin.macaroon; installing a placeholder, payments stay disabled." >&2
              head -c 32 /dev/urandom > ${containerState}/secrets/admin.macaroon
            fi
          '';
          serviceConfig = {
            User = "proofofscore";
            Group = "proofofscore";
            WorkingDirectory = containerState;
            ExecStart = "${server}/bin/server -c ${configuration}";
            Restart = "on-failure";
            RestartSec = 5;
            NoNewPrivileges = true;
            ProtectSystem = "strict";
            ProtectHome = true;
            PrivateTmp = true;
            ProtectKernelTunables = true;
            ProtectKernelModules = true;
            ProtectControlGroups = true;
            RestrictAddressFamilies = [ "AF_UNIX" "AF_INET" "AF_INET6" ];
            ReadWritePaths = [ "${containerState}/data" "${containerState}/creds" "${containerState}/secrets" ];
          };
        };
      };
    };
    systemd.services."container@proofofscore" = {
      requires = [ "${cfg.storage.prepareService}.service" ];
      after = [ "${cfg.storage.prepareService}.service" ];
      bindsTo = lib.optional (cfg.storage.mountUnit != null) cfg.storage.mountUnit;
      unitConfig.RequiresMountsFor = stateDir;
      serviceConfig.Slice = lib.mkForce cfg.storage.slice;
    };
    networking.nat = {
      enable = lib.mkIf cfg.manageGateway true;
      internalInterfaces = [ "ve-proofofscore" ];
    };
    networking.nftables.enable = lib.mkIf cfg.manageGateway true;
    networking.firewall = lib.mkIf cfg.manageGateway {
      backend = "nftables";
      allowedTCPPorts = lib.optionals cfg.gateway.openFirewall [ cfg.gateway.httpPort cfg.gateway.httpsPort ];
      extraForwardRules = ''
        iifname "ve-proofofscore" ip saddr ${containerAddress} accept
      '';
    };

    # Virtual hosts also integrate with an existing shared Caddy instance.
    services.caddy = {
      enable = lib.mkIf cfg.manageGateway true;
      globalConfig = lib.mkIf cfg.manageGateway ''
        http_port ${toString cfg.gateway.httpPort}
        https_port ${toString cfg.gateway.httpsPort}
      '';
      virtualHosts = lib.optionalAttrs operatorEnabled {
        ${operator.domain} = {
          extraConfig = lib.optionalString (operator.acmeHost == null) "tls internal\n" + ''
            @operator remote_ip ${lib.concatStringsSep " " operator.allowedCIDRs}
            handle @operator {
              reverse_proxy ${containerAddress}:${toString port}
            }
            respond 403
          '';
        } // lib.optionalAttrs (operator.acmeHost != null) { useACMEHost = operator.acmeHost; };
      } // {
        "www.${domain}".extraConfig = ''
          redir https://${domain}{uri} permanent
        '';
        ${domain}.extraConfig = ''
          handle /admin* {
            respond 403
          }
          handle {
            @hashed_assets {
              path_regexp \.(css|js)$
              path /static/*
            }
            header @hashed_assets Cache-Control "public, max-age=31536000, immutable"
            reverse_proxy ${containerAddress}:${toString port}
          }
        '';
      };
    };

    # A consistent SQLite snapshot for the applications backup job.
    systemd.services.${cfg.storage.exportService} = {
      description = "Export the Proof of Score database for backups";
      unitConfig.RequiresMountsFor = stateDir;
      serviceConfig.Type = "oneshot";
      path = [ pkgs.sqlite pkgs.coreutils ];
      script = ''
        set -eu
        [ -s ${lib.escapeShellArg "${stateDir}/data/game.db"} ] || exit 0
        install -d -m 0700 ${lib.escapeShellArg cfg.storage.exportDir}
        sqlite3 ${lib.escapeShellArg "${stateDir}/data/game.db"} ${lib.escapeShellArg (".backup '" + lib.replaceStrings [ "'" ] [ "''" ] "${cfg.storage.exportDir}/proofofscore.db.tmp" + "'")}
        mv ${lib.escapeShellArg "${cfg.storage.exportDir}/proofofscore.db.tmp"} ${lib.escapeShellArg "${cfg.storage.exportDir}/proofofscore.db"}
      '';
    };
    systemd.timers.${cfg.storage.exportService} = {
      wantedBy = [ "timers.target" ];
      timerConfig = { OnCalendar = "*-*-* 02:30:00"; Persistent = true; };
    };
  };
}
