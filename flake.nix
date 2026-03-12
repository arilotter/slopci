{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    crane.url = "github:ipetkov/crane";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, crane, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
        craneLib = crane.mkLib pkgs;

        src = craneLib.cleanCargoSource ./.;

        commonArgs = {
          inherit src;
          strictDeps = true;
          buildInputs = with pkgs; [
            openssl
            sqlite
          ] ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
            pkgs.darwin.apple_sdk.frameworks.Security
            pkgs.darwin.apple_sdk.frameworks.SystemConfiguration
          ];
          nativeBuildInputs = with pkgs; [
            pkg-config
          ];
        };

        cargoArtifacts = craneLib.buildDepsOnly commonArgs;

        nixci = craneLib.buildPackage (commonArgs // {
          inherit cargoArtifacts;
          pname = "nixci";
          cargoExtraArgs = "--package nixci";
        });

        nixci-secrets = craneLib.buildPackage (commonArgs // {
          inherit cargoArtifacts;
          pname = "nixci-secrets";
          cargoExtraArgs = "--package nixci-secrets";
        });
      in
      {
        packages = {
          inherit nixci nixci-secrets;
          default = nixci;
        };

        checks = {
          inherit nixci nixci-secrets;
        };

        devShells.default = craneLib.devShell {
          packages = with pkgs; [
            cargo-watch
            sqlx-cli
            nix-fast-build
          ];
          inputsFrom = [ nixci ];
        };
      }
    ) // {
      nixosModules.default = { config, lib, pkgs, ... }:
        let
          cfg = config.services.nixci;
        in
        {
          options.services.nixci = {
            enable = lib.mkEnableOption "nixci CI service";

            listenAddr = lib.mkOption {
              type = lib.types.str;
              default = "127.0.0.1:3000";
              description = "Address to listen on";
            };

            databasePath = lib.mkOption {
              type = lib.types.str;
              default = "/var/lib/nixci/nixci.db";
              description = "Path to SQLite database";
            };

            workDir = lib.mkOption {
              type = lib.types.str;
              default = "/var/lib/nixci/work";
              description = "Working directory for repo clones";
            };

            githubAppId = lib.mkOption {
              type = lib.types.int;
              description = "GitHub App ID";
            };

            githubPrivateKeyFile = lib.mkOption {
              type = lib.types.str;
              description = "Path to GitHub App private key PEM file";
            };

            githubWebhookSecretFile = lib.mkOption {
              type = lib.types.str;
              description = "Path to file containing GitHub webhook secret";
            };

            maxConcurrentBuilds = lib.mkOption {
              type = lib.types.int;
              default = 4;
              description = "Maximum number of concurrent builds";
            };

            secrets = {
              enable = lib.mkEnableOption "nixci-secrets service";

              masterKeyFile = lib.mkOption {
                type = lib.types.str;
                description = "Path to master key file for secret derivation";
              };

              socketPath = lib.mkOption {
                type = lib.types.str;
                default = "/run/nixci-secrets/nixci-secrets.sock";
                description = "Unix socket path for nixci-secrets";
              };
            };

            package = lib.mkOption {
              type = lib.types.package;
              default = self.packages.${pkgs.system}.nixci;
              description = "nixci package to use";
            };

            secretsPackage = lib.mkOption {
              type = lib.types.package;
              default = self.packages.${pkgs.system}.nixci-secrets;
              description = "nixci-secrets package to use";
            };
          };

          config = lib.mkIf cfg.enable {
            systemd.services.nixci = {
              description = "nixci CI service";
              wantedBy = [ "multi-user.target" ];
              after = [ "network.target" ] ++ lib.optionals cfg.secrets.enable [ "nixci-secrets.service" ];
              wants = lib.optionals cfg.secrets.enable [ "nixci-secrets.service" ];

              serviceConfig = {
                ExecStart = "${cfg.package}/bin/nixci";
                DynamicUser = true;
                StateDirectory = "nixci";
                RuntimeDirectory = "nixci";
                EnvironmentFile = [ ];
              };

              environment = {
                NIXCI_LISTEN = cfg.listenAddr;
                NIXCI_DATABASE_URL = "sqlite:${cfg.databasePath}";
                NIXCI_WORK_DIR = cfg.workDir;
                NIXCI_GITHUB_APP_ID = toString cfg.githubAppId;
                NIXCI_GITHUB_PRIVATE_KEY = cfg.githubPrivateKeyFile;
                NIXCI_MAX_CONCURRENT_BUILDS = toString cfg.maxConcurrentBuilds;
                NIXCI_SECRETS_SOCKET = cfg.secrets.socketPath;
              };

              preStart = ''
                export NIXCI_GITHUB_WEBHOOK_SECRET="$(cat ${cfg.githubWebhookSecretFile})"
              '';
            };

            systemd.services.nixci-secrets = lib.mkIf cfg.secrets.enable {
              description = "nixci secrets service";
              wantedBy = [ "multi-user.target" ];

              serviceConfig = {
                ExecStart = "${cfg.secretsPackage}/bin/nixci-secrets";
                DynamicUser = true;
                RuntimeDirectory = "nixci-secrets";
              };

              environment = {
                NIXCI_SECRETS_MASTER_KEY = cfg.secrets.masterKeyFile;
                NIXCI_SECRETS_SOCKET = cfg.secrets.socketPath;
              };
            };
          };
        };
    };
}
