{ nixpkgs, system ? "x86_64-linux" }:

let
  inherit (nixpkgs) lib;
  pkgs = nixpkgs.legacyPackages.${system};
  evaluate = modules: (lib.nixosSystem {
    inherit system;
    modules = [
      ./module.nix
      {
        boot.isContainer = true;
        system.stateVersion = "26.05";
      }
    ] ++ modules;
  }).config;
  enabledModule = {
    services.proofofscore = {
      enable = true;
      domain = "scores.example.test";
      package = pkgs.emptyDirectory;
      lndUrl = "https://payments.example.test:8080";
    };
  };
  standalone = evaluate [ enabledModule ];
  disabled = evaluate [ { } ];
  shared = evaluate [ enabledModule {
    services.proofofscore = {
      manageGateway = false;
      stateDir = "/srv/games/proofofscore";
      hostAddress = "10.92.0.1";
      containerAddress = "10.92.0.2";
      operator = {
        domain = "operator.example.test";
        allowedCIDRs = [ "192.0.2.4/32" ];
      };
      storage = {
        prepareService = "shared-storage";
        managePrepareService = false;
        mountUnit = "srv-games.mount";
        slice = "games.slice";
        exportDir = "/srv/games/exports";
        exportService = "shared-proofofscore-export";
      };
    };
    systemd.services.shared-storage = {
      script = "echo shared preparation";
      serviceConfig.Type = "oneshot";
    };
  } ];
  deniedOperator = evaluate [ enabledModule {
    services.proofofscore.operator.domain = "operator.example.test";
  } ];
  publicHost = standalone.services.caddy.virtualHosts."scores.example.test".extraConfig;
  operatorHost = shared.services.caddy.virtualHosts."operator.example.test".extraConfig;
  checks = {
    disabledCreatesNoServices = !(disabled.containers ? proofofscore)
      && !disabled.services.caddy.enable
      && !(disabled.systemd.services ? proofofscore-storage);
    standaloneEvaluates = lib.all (assertion: assertion.assertion) standalone.assertions;
    gatewayNeedsExplicitExposure = standalone.services.caddy.enable
      && standalone.networking.nat.enable
      && standalone.networking.firewall.allowedTCPPorts == [ ]
      && lib.hasInfix "ve-proofofscore" standalone.networking.firewall.extraForwardRules;
    publicAdminRemainsBlocked = lib.hasInfix "handle /admin* {\n  respond 403\n}" publicHost
      && lib.hasInfix "reverse_proxy 10.91.0.2:8900" publicHost;
    operatorRequiresExplicitSources = lib.hasInfix "@operator remote_ip 192.0.2.4/32" operatorHost
      && lib.hasInfix "respond 403" operatorHost
      && lib.any (assertion: !assertion.assertion && lib.hasInfix "allowedCIDRs" assertion.message) deniedOperator.assertions;
    sharedGatewayRemainsHostOwned = !shared.services.caddy.enable
      && !shared.networking.nat.enable
      && shared.networking.firewall.allowedTCPPorts == [ ]
      && shared.networking.nat.internalInterfaces == [ "ve-proofofscore" ];
    stateAndUnitMappingSurvive = shared.containers.proofofscore.bindMounts."/var/lib/proofofscore".hostPath == "/srv/games/proofofscore"
      && shared.systemd.services."container@proofofscore".requires == [ "shared-storage.service" ]
      && shared.systemd.services."container@proofofscore".bindsTo == [ "srv-games.mount" ]
      && shared.systemd.services."container@proofofscore".serviceConfig.Slice == "games.slice"
      && lib.hasInfix "echo shared preparation" shared.systemd.services.shared-storage.script;
    exportsKeepStableFilename = lib.hasInfix "/srv/games/exports/proofofscore.db" shared.systemd.services.shared-proofofscore-export.script
      && shared.systemd.timers.shared-proofofscore-export.timerConfig.OnCalendar == "*-*-* 02:30:00";
    missingMacaroonKeepsPlaceholder = lib.hasInfix "head -c 32 /dev/urandom" standalone.containers.proofofscore.config.systemd.services.proofofscore.preStart
      && lib.hasInfix "/var/lib/proofofscore/secrets/admin.macaroon" standalone.containers.proofofscore.config.systemd.services.proofofscore.preStart;
  };
in
{
  inherit checks;
  derivation = assert lib.assertMsg (lib.all (value: value) (builtins.attrValues checks))
    "Proof of Score module policy failed: ${lib.concatStringsSep ", " (builtins.attrNames (lib.filterAttrs (_: value: !value) checks))}";
    pkgs.runCommand "proofofscore-nixos-module-policy" { } ''
      touch "$out"
    '';
}
