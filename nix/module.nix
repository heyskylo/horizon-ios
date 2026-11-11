# services.horizon-server: a Horizon server on NixOS (docs/nix.md).
#
# Horizon ships none of Mojang's files, and neither does this module: the game files come from
# horizon-prepare run on your own copy of Minecraft 26.3 and the 26.3 server jar you downloaded yourself.
# Either point `resources` at a folder you prepared elsewhere, or let `prepare` run horizon-prepare here
# on files you put on this machine. Nothing here downloads Minecraft; the prepare step even runs without
# network access.
{ self }:
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.services.horizon-server;
  system = pkgs.stdenv.hostPlatform.system;
  inherit (lib)
    mkOption
    mkEnableOption
    mkIf
    types
    escapeShellArg
    escapeShellArgs
    ;

  # Absolute paths kept as strings: a Nix path would copy the game files into the world-readable store.
  absolute = types.strMatching "/.*";

  defaultDataDir = "/var/lib/horizon-server";
  preparedDir = "${cfg.dataDir}/game-files";
  resources = if cfg.resources != null then cfg.resources else preparedDir;

  settingValue = v: if lib.isBool v then lib.boolToString v else toString v;
  settings = {
    "server.port" = cfg.port;
    "engine.resources" = resources;
  }
  // cfg.settings;
  settingArgs = lib.mapAttrsToList (k: v: "${k}=${settingValue v}") settings;

  initArgs = [
    "--eula"
    "--yes"
    "--port"
    (toString cfg.port)
    "--world"
    cfg.init.world
  ]
  ++ lib.optionals (cfg.init.seed != null) [
    "--seed"
    cfg.init.seed
  ]
  ++ [ (if cfg.init.whitelist then "--whitelist" else "--no-whitelist") ]
  ++ lib.concatMap (p: [
    "--player"
    p
  ]) cfg.init.players
  ++ lib.optionals (cfg.init.op != null) [
    "--op"
    cfg.init.op
  ]
  ++ lib.optional (!cfg.init.onlineMode) "--offline";

  server = lib.getExe cfg.package;

  # As the service's user, before every start: the first start writes server.toml (`init`), every start
  # sets what this module manages in it (`config set`: checked first, comments kept).
  configure = pkgs.writeShellScript "horizon-server-configure" ''
    set -eu
    if [ ! -f ${escapeShellArg cfg.dataDir}/server.toml ]; then
      ${server} --dir ${escapeShellArg cfg.dataDir} init ${escapeShellArgs initArgs}
    fi
    ${server} --dir ${escapeShellArg cfg.dataDir} config set ${escapeShellArgs settingArgs}
  '';

  prepareStamp = "${preparedDir}/.nix-prepared";
  prepareKey = "${cfg.prepare.package} ${cfg.prepare.serverJar} ${toString cfg.prepare.minecraftDir}";
  prepareArgs = [
    "--prepare"
    "--server-jar"
    cfg.prepare.serverJar
    "--output"
    preparedDir
  ]
  ++ lib.optionals (cfg.prepare.minecraftDir != null) [
    "--minecraft-dir"
    cfg.prepare.minecraftDir
  ]
  ++ cfg.prepare.extraArgs;
  prepare = pkgs.writeShellScript "horizon-server-prepare" ''
    set -eu
    key=${escapeShellArg prepareKey}
    if [ -f ${escapeShellArg preparedDir}/horizon-resources.json ] \
      && [ "$(cat ${escapeShellArg prepareStamp} 2>/dev/null)" = "$key" ]; then
      echo "Game files already prepared by this horizon-prepare from this jar"
      exit 0
    fi
    rm -f ${escapeShellArg prepareStamp}
    ${lib.getExe cfg.prepare.package} ${escapeShellArgs prepareArgs}
    printf '%s' "$key" > ${escapeShellArg prepareStamp}
  '';

  hardening = {
    NoNewPrivileges = true;
    CapabilityBoundingSet = "";
    AmbientCapabilities = "";
    ProtectSystem = "strict";
    ProtectHome = true;
    PrivateTmp = true;
    PrivateDevices = true;
    PrivateUsers = true;
    ProtectHostname = true;
    ProtectClock = true;
    ProtectKernelTunables = true;
    ProtectKernelModules = true;
    ProtectKernelLogs = true;
    ProtectControlGroups = true;
    ProtectProc = "invisible";
    RestrictNamespaces = true;
    RestrictRealtime = true;
    RestrictSUIDSGID = true;
    LockPersonality = true;
    RemoveIPC = true;
    SystemCallArchitectures = "native";
    SystemCallFilter = [
      "@system-service"
      "~@privileged"
    ];
    SystemCallErrorNumber = "EPERM";
    UMask = "0077";
    ReadWritePaths = [ cfg.dataDir ];
  };
in
{
  options.services.horizon-server = {
    enable = mkEnableOption "a Horizon server (Minecraft: Java Edition 26.3, rewritten in Rust)";

    package = mkOption {
      type = types.package;
      default = self.packages.${system}.horizon-server;
      defaultText = lib.literalExpression "horizon.packages.\${system}.horizon-server";
      description = "The horizon-server package; its engine (horizon-engine) sits beside it.";
    };

    eula = mkOption {
      type = types.bool;
      default = false;
      description = ''
        Whether you agree to the Minecraft EULA (https://aka.ms/MinecraftEULA) for this server. The
        service refuses to start without it, as vanilla's server does.
      '';
    };

    dataDir = mkOption {
      type = absolute;
      default = defaultDataDir;
      description = "The server folder: server.toml, the world, logs and backups.";
    };

    user = mkOption {
      type = types.str;
      default = "horizon-server";
      description = "The user the server runs as; created when left at the default.";
    };

    group = mkOption {
      type = types.str;
      default = "horizon-server";
      description = "The group the server runs as; created when left at the default.";
    };

    port = mkOption {
      type = types.port;
      default = 25565;
      description = "The port players join on (server.toml `server.port`).";
    };

    openFirewall = mkOption {
      type = types.bool;
      default = false;
      description = "Whether to open `port` (TCP) in the firewall.";
    };

    resources = mkOption {
      type = types.nullOr absolute;
      default = null;
      example = "/srv/horizon/26.3";
      description = ''
        A folder of game files you prepared yourself (`horizon-prepare --prepare`, or the launcher; the
        `26.3` folder it writes). Read only. Null: the files `prepare` writes to `dataDir/game-files`.
        Give it as a string: a Nix path would copy Mojang's files into the world-readable store.
      '';
    };

    prepare = {
      enable = mkEnableOption ''
        running horizon-prepare on this machine, before the server starts, from your own Minecraft 26.3
        files. It runs once per horizon-prepare package and jar, without network access'';

      package = mkOption {
        type = types.package;
        default = self.packages.${system}.horizon-prepare;
        defaultText = lib.literalExpression "horizon.packages.\${system}.horizon-prepare";
        description = "The horizon-prepare package, with the JDK its exporters compile with.";
      };

      serverJar = mkOption {
        type = absolute;
        example = "/srv/horizon/server.jar";
        description = ''
          The 26.3 `server.jar` you downloaded yourself (`downloads.server.url` in your launcher's
          `versions/26.3/26.3.json`). Its hash is checked against the one Horizon pins.
        '';
      };

      minecraftDir = mkOption {
        type = types.nullOr absolute;
        default = null;
        example = "/srv/horizon/minecraft";
        description = ''
          A copy of the official launcher's game folder with 26.3 played once (`versions/26.3`,
          `libraries`, `assets`): the client side horizon-prepare also reads. Must be readable by `user`.
        '';
      };

      extraArgs = mkOption {
        type = types.listOf types.str;
        default = [ ];
        example = [
          "--client-jar"
          "/srv/horizon/client.jar"
        ];
        description = "More horizon-prepare arguments (`horizon-prepare --help`).";
      };
    };

    init = {
      world = mkOption {
        type = types.str;
        default = "world";
        description = "The world's folder name, for the first start.";
      };
      seed = mkOption {
        type = types.nullOr types.str;
        default = null;
        description = "The world's seed, for the first start; null: a random one.";
      };
      whitelist = mkOption {
        type = types.bool;
        default = true;
        description = "Whether the whitelist is on, for the first start (vanilla's default: on).";
      };
      players = mkOption {
        type = types.listOf types.str;
        default = [ ];
        description = "Players to whitelist on the first start.";
      };
      op = mkOption {
        type = types.nullOr types.str;
        default = null;
        description = "A player to make an operator on the first start.";
      };
      onlineMode = mkOption {
        type = types.bool;
        default = true;
        description = ''
          Whether to check accounts with Mojang, for the first start. An offline server lets anyone in
          under any name: keep it on your LAN or a tailnet.
        '';
      };
    };

    settings = mkOption {
      type = types.attrsOf (
        types.oneOf [
          types.bool
          types.int
          types.str
        ]
      );
      default = { };
      example = {
        "world.difficulty" = "hard";
        "server.max_players" = 10;
        "properties.spawn-protection" = 0;
      };
      description = ''
        server.toml settings set before every start with `horizon-server config set` (section.key, as in
        docs/hosting.md), which checks them first and keeps the rest of the file, comments included.
        Settings not named here stay yours to edit. `server.port` and `engine.resources` come from
        `port` and `resources`.
      '';
    };

    environment = mkOption {
      type = types.attrsOf types.str;
      default = { };
      example = {
        HORIZON_SPIKES = "100";
      };
      description = "More environment variables for the server.";
    };
  };

  config = mkIf cfg.enable {
    assertions = [
      {
        assertion = cfg.eula;
        message = "services.horizon-server.eula: agree to the Minecraft EULA (https://aka.ms/MinecraftEULA) to run a server.";
      }
      {
        assertion = cfg.resources != null || cfg.prepare.enable;
        message = "services.horizon-server: set `resources` to game files you prepared, or enable `prepare`.";
      }
      {
        assertion = !(cfg.resources != null && cfg.prepare.enable);
        message = "services.horizon-server: `resources` and `prepare` both give the game files: use one.";
      }
    ];

    users.users = mkIf (cfg.user == "horizon-server") {
      horizon-server = {
        isSystemUser = true;
        inherit (cfg) group;
        home = cfg.dataDir;
        description = "Horizon server";
      };
    };
    users.groups = mkIf (cfg.group == "horizon-server") { horizon-server = { }; };

    networking.firewall.allowedTCPPorts = mkIf cfg.openFirewall [ cfg.port ];

    # `horizon-server --dir DIR status|cmd|logs|backup ...` for the administrator (as `user`).
    environment.systemPackages = [ cfg.package ];

    systemd = {
      tmpfiles.settings.horizon-server = mkIf (cfg.dataDir != defaultDataDir) {
        ${cfg.dataDir}.d = {
          inherit (cfg) user group;
          mode = "0700";
        };
      };

      services.horizon-server-prepare = mkIf cfg.prepare.enable {
        description = "Prepare Horizon's game files from your own Minecraft 26.3";
        before = [ "horizon-server.service" ];
        requiredBy = [ "horizon-server.service" ];
        unitConfig.RequiresMountsFor = [
          cfg.dataDir
        ]
        ++ lib.optional (cfg.prepare.minecraftDir != null) cfg.prepare.minecraftDir;
        environment = {
          HOME = cfg.dataDir;
          HORIZON_PREPARE_ROOT = "${cfg.dataDir}/.prepare";
        };
        serviceConfig = hardening // {
          Type = "oneshot";
          RemainAfterExit = true;
          User = cfg.user;
          Group = cfg.group;
          StateDirectory = mkIf (cfg.dataDir == defaultDataDir) "horizon-server";
          StateDirectoryMode = "0700";
          WorkingDirectory = cfg.dataDir;
          ExecStart = prepare;
          # Proof that it fetches nothing: the preparation has no network at all.
          PrivateNetwork = true;
          RestrictAddressFamilies = [ "AF_UNIX" ];
          ReadOnlyPaths = [
            cfg.prepare.serverJar
          ]
          ++ lib.optional (cfg.prepare.minecraftDir != null) cfg.prepare.minecraftDir;
        };
      };

      services.horizon-server = {
        description = "Horizon server (Minecraft: Java Edition 26.3)";
        wantedBy = [ "multi-user.target" ];
        after = [ "network-online.target" ];
        wants = [ "network-online.target" ];
        unitConfig.RequiresMountsFor = [
          cfg.dataDir
        ]
        ++ lib.optional (cfg.resources != null) cfg.resources;
        environment = {
          HOME = cfg.dataDir;
          # Nix decides the release: no "a newer release is out" in the log, and no audio device.
          HORIZON_NO_UPDATE_CHECK = "1";
          HORIZON_NO_AUDIO = "1";
        }
        // cfg.environment;
        serviceConfig = hardening // {
          Type = "simple";
          User = cfg.user;
          Group = cfg.group;
          StateDirectory = mkIf (cfg.dataDir == defaultDataDir) "horizon-server";
          StateDirectoryMode = "0700";
          WorkingDirectory = cfg.dataDir;
          ExecStartPre = configure;
          ExecStart = "${server} --dir ${escapeShellArg cfg.dataDir} run";
          # SIGTERM makes horizon-server save the world and stop the game; give it the time.
          KillSignal = "SIGTERM";
          KillMode = "mixed";
          TimeoutStopSec = "180s";
          Restart = "on-failure";
          RestartSec = "10s";
          RestrictAddressFamilies = [
            "AF_INET"
            "AF_INET6"
            "AF_UNIX"
          ];
          MemoryDenyWriteExecute = true;
          ReadOnlyPaths = lib.optional (cfg.resources != null) cfg.resources;
        };
      };
    };
  };
}
