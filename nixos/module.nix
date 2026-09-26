{ config, lib, pkgs, ... }:

# A self-contained systemd-nspawn deployment. The public proxy blocks /admin;
# an optional operator hostname admits only explicitly configured sources.
let
  cfg = config.services.proofofscore;
  inherit (cfg) stateDir hostAddress containerAddress port account;
  domain = if cfg.domain == null then "proofofscore.invalid" else cfg.domain;
  # With nix-rollout, each slot runs the build the controller placed in its
  # directory, bind-mounted here, and the proxy follows the controller's
  # upstream file. One slot runs at a time: two servers must not share game.db.
  rollout = cfg.rollout;
  slotDir = "/var/lib/nix-rollout-slot";
  system = pkgs.stdenv.hostPlatform.system;
  # A published release: the archive for this host's system, as released,
  # with its executable patched to run against this system's C runtime.
  release = cfg.release;
  released = pkgs.stdenv.mkDerivation {
    pname = "proofofscore";
    inherit (release) version;
    src = pkgs.fetchurl {
      url = "https://github.com/tee8z/proofofscore/releases/download/v${release.version}/proofofscore-${release.version}-${system}.tar.gz";
      inherit (release) sha256;
    };
    nativeBuildInputs = [ pkgs.autoPatchelfHook ];
    buildInputs = [ pkgs.stdenv.cc.cc.lib ];
    dontConfigure = true;
    dontBuild = true;
    installPhase = ''
      runHook preInstall
      if [ "$(cat share/proofofscore/REVISION)" != ${lib.escapeShellArg release.revision} ]; then
        echo "proofofscore ${release.version} was built from $(cat share/proofofscore/REVISION), not ${release.revision}" >&2
        exit 1
      fi
      mkdir -p $out
      cp -R bin share $out/
      install -Dm0644 LICENSE $out/share/licenses/proofofscore/LICENSE
      runHook postInstall
    '';
  };
  package = if release != null then released else cfg.package;
  server = if rollout != null then "${slotDir}/artifact" else package;
  # A release ships the browser modules; other packages leave them to
  # stateDir/ui.
  uiDir = if release != null then "${server}/share/proofofscore/ui" else "${containerState}/ui";
  containerState = "/var/lib/proofofscore";
  # One container, proofofscore, or with slots one per slot, pos-<slot>.
  instances = if cfg.slots == { } then [{
    container = "proofofscore";
    slot = null;
    inherit hostAddress containerAddress;
  }] else lib.mapAttrsToList (slot: value: {
    container = "pos-${slot}";
    inherit slot;
    inherit (value) hostAddress containerAddress;
  }) cfg.slots;
  upstream = if rollout != null then ''
    reverse_proxy {
      import ${rollout}/upstream*.caddy ${toString port}
    }'' else "reverse_proxy ${containerAddress}:${toString port}";
  operator = cfg.operator;
  operatorEnabled = operator.domain != null;
  configurationFor = instance: pkgs.writeText "proofofscore.toml" ''
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
    ui_dir = "${uiDir}"
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
    allowed_subnets = ["${instance.hostAddress}/32", "127.0.0.1/32"]
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
    release = mkOption {
      type = types.nullOr (types.submodule {
        options = {
          version = mkOption { type = types.strMatching "[0-9]+\\.[0-9]+\\.[0-9]+(-[0-9A-Za-z.-]+)?"; example = "0.3.1"; description = "Released version, without the v of its tag."; };
          sha256 = mkOption { type = types.str; description = "SHA-256 of proofofscore-<version>-<system>.tar.gz for this host's system, from its .sha256 asset."; };
          revision = mkOption { type = types.strMatching "[0-9a-f]{40}"; description = "Source commit the release was built from; the archive's share/proofofscore/REVISION must match."; };
        };
      });
      default = null;
      example = { version = "0.3.1"; sha256 = "<64 hex digits>"; revision = "<40 hex digits>"; };
      description = ''
        Run this published GitHub release instead of building from source. The module fetches the
        archive for the host's system and serves its browser modules from share/proofofscore/ui.
        Null runs `package`.
      '';
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
    slots = mkOption {
      type = types.attrsOf (types.submodule {
        options = {
          hostAddress = mkOption { type = types.str; description = "Host-side IPv4 address of the slot's container link."; };
          containerAddress = mkOption { type = types.str; description = "Container-side IPv4 address of the slot's container link."; };
        };
      });
      default = { };
      description = "Two containers, pos-<slot>, on the same state, for nix-rollout. Empty runs one container, proofofscore.";
    };
    rollout = mkOption {
      type = types.nullOr (types.strMatching "/[A-Za-z0-9_./-]+");
      default = null;
      description = ''
        nix-rollout runtime directory for the two slots, such as /var/lib/nix-rollout/apps/proofofscore.
        Each slot then runs the build nix-rollout placed in <rollout>/slots/<slot>/artifact and the proxy
        follows <rollout>/upstream.caddy. Deploy with the recreate strategy: one server at a time.
      '';
    };
    artifact = mkOption {
      type = types.package;
      readOnly = true;
      description = "The server package as the one store path nix-rollout deploys into a slot.";
    };
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

  config = lib.mkMerge [
  # Outside the mkIf: its condition reads this module's options.
  { services.proofofscore.artifact = lib.mkIf (release != null || cfg.package != null) package; }
  (lib.mkIf cfg.enable {
    systemd.services = lib.listToAttrs (map (instance: lib.nameValuePair "container@${instance.container}" {
      requires = [ "${cfg.storage.prepareService}.service" ];
      after = [ "${cfg.storage.prepareService}.service" ];
      bindsTo = lib.optional (cfg.storage.mountUnit != null) cfg.storage.mountUnit;
      unitConfig.RequiresMountsFor = stateDir;
      serviceConfig.Slice = lib.mkForce cfg.storage.slice;
    }) instances);
  })
  (lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = cfg.domain != null && builtins.match "[a-z0-9]([a-z0-9-]*[a-z0-9])?(\\.[a-z0-9]([a-z0-9-]*[a-z0-9])?)+" domain != null;
        message = "services.proofofscore.domain must be a lowercase DNS hostname.";
      }
      { assertion = release != null || cfg.package != null; message = "Set services.proofofscore.release or package, or import the application's flake module."; }
      { assertion = rollout == null || builtins.length instances == 2; message = "services.proofofscore.rollout needs exactly two slots."; }
      { assertion = cfg.slots == { } || lib.all (instance: builtins.stringLength instance.container <= 11) instances; message = "Proof of Score slot names must be at most 7 characters."; }
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

    containers = lib.listToAttrs (map (instance: lib.nameValuePair instance.container {
      autoStart = true;
      privateNetwork = true;
      inherit (instance) hostAddress;
      localAddress = instance.containerAddress;
      bindMounts = {
        ${containerState} = { hostPath = stateDir; isReadOnly = false; };
      } // lib.optionalAttrs (rollout != null) {
        ${slotDir} = { hostPath = "${rollout}/slots/${instance.slot}"; isReadOnly = true; };
      };
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
            ExecStart = "${server}/bin/server -c ${configurationFor instance}";
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
    }) instances);
    networking.nat = {
      enable = lib.mkIf cfg.manageGateway true;
      internalInterfaces = map (instance: "ve-${instance.container}") instances;
    };
    networking.nftables.enable = lib.mkIf cfg.manageGateway true;
    networking.firewall = lib.mkIf cfg.manageGateway {
      backend = "nftables";
      allowedTCPPorts = lib.optionals cfg.gateway.openFirewall [ cfg.gateway.httpPort cfg.gateway.httpsPort ];
      extraForwardRules = lib.concatMapStringsSep "\n" (instance:
        "iifname \"ve-${instance.container}\" ip saddr ${instance.containerAddress} accept") instances;
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
              ${upstream}
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
            ${upstream}
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
  })
  ];
}
