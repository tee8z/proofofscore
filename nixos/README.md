# NixOS deployment

The module runs Proof of Score in a private systemd container with persistent host storage.
It owns the application service, public route restrictions, optional operator route, and nightly SQLite export.

Import `proofofscore.nixosModules.default` or `proofofscore.nixosModules.proofofscore` from the flake input.
Both exports select the server package with its generated static assets and migrations.

```nix
{
  imports = [ inputs.proofofscore.nixosModules.default ];

  services.proofofscore = {
    enable = true;
    domain = "scores.example.org";
    lndUrl = "https://payments.example.org:8080";
    gateway.openFirewall = true;

    operator = {
      domain = "operator.example.org";
      allowedCIDRs = [ "192.0.2.4/32" ];
    };
  };
}
```

Replace the example hostname, endpoint, and operator source before deployment.

To run a published release instead of a source build, set `release`.
Take `sha256` from the `proofofscore-<version>-<system>.tar.gz.sha256` asset for the host's system, and `revision` from the release's tag commit:

```nix
services.proofofscore.release = {
  version = "0.3.1";
  sha256 = "<the archive's SHA-256>";
  revision = "<the tagged commit>";
};
```

The module fetches the archive, patches the executable for the host's C runtime, and checks that the archive was built from `revision`.
It also serves the browser modules from the archive's `share/proofofscore/ui`; without a release they come from `stateDir/ui`.
A release takes precedence over `package`.
The module enables Caddy and container NAT by default.
Opening the host HTTP and HTTPS ports requires `gateway.openFirewall = true`.

The public hostname blocks `/admin*`.
The optional operator hostname requires explicit source CIDRs.
It uses Caddy's internal certificate authority unless `operator.acmeHost` names an existing NixOS ACME certificate.
Without an operator hostname, the module adds no operator route.

Persistent files stay at `/var/lib/proofofscore` inside the container.
The host path defaults to the same name and can change through `stateDir`.
Place the LND macaroon at `stateDir/secrets/admin.macaroon` with ownership matching `account`, which defaults to `61200`.
Without a macaroon, startup writes a random placeholder and payments remain unavailable.
The application signing key path remains `/var/lib/proofofscore/creds/private.pem`.

For a shared host gateway, set `manageGateway = false`.
The module still contributes Caddy virtual hosts and the `ve-proofofscore` NAT interface.
The host must enable Caddy, configure its listeners, and permit outbound container traffic.

| Option | Default | Host integration |
| --- | --- | --- |
| `hostAddress`, `containerAddress` | `10.91.0.1`, `10.91.0.2` | Private container IPv4 addresses |
| `port`, `account` | `8900`, `61200` | Container HTTP port and numeric UID/GID |
| `storage.prepareService` | `proofofscore-storage` | Service basename, without `.service` |
| `storage.managePrepareService` | `true` | Set false to append directory creation to an existing service |
| `storage.mountUnit` | `null` | Optional mount unit that stops the container when removed |
| `storage.slice` | `system.slice` | Container resource slice |
| `storage.exportDir` | `/var/lib/proofofscore-backups` | Destination for `proofofscore.db` |
| `storage.exportService` | `proofofscore-export` | Nightly export service and timer basename |

The export uses SQLite's backup command at 02:30 host time.
Transfer the export and signing credentials to independent backup storage separately.

The raw entry point is `nixos/module.nix` and has no other module dependencies.
When importing this file directly, set `services.proofofscore.package` explicitly.
The package must contain `bin/server` and `share/proofofscore/{static,migrations}`.
For an older package that installs only the executable, apply `nixos/package.nix` with `{ package = yourPinnedPackage; }`.
This helper adds the generated static assets and migrations without changing the application revision or Rust toolchain.

Run the module policy check without building the application:

```sh
nix build .#checks.x86_64-linux.nixos-module
```

The check evaluates standalone, disabled, and shared gateway configurations with a substitute package.
It checks route restrictions, storage mappings, credential placeholder behavior, and explicit firewall exposure.
