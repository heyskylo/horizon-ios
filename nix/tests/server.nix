# services.horizon-server in a VM, without any of Mojang's files: the module writes server.toml on the
# first start, keeps the settings it manages at every start, opens the firewall, and the server stops with
# its own "no prepared game files" message instead of running on nothing.
{ self }:
{
  name = "horizon-server";

  nodes.machine =
    { ... }:
    {
      imports = [ self.nixosModules.horizon-server ];
      services.horizon-server = {
        enable = true;
        eula = true;
        port = 25570;
        openFirewall = true;
        # An empty folder stands in for the prepared files, which a test can't have.
        resources = "/srv/horizon/26.3";
        init = {
          world = "test";
          seed = "12345";
          onlineMode = false;
          players = [ "Alex" ];
          op = "Alex";
        };
        settings = {
          "world.difficulty" = "hard";
          "server.max_players" = 7;
        };
      };
      systemd.tmpfiles.rules = [ "d /srv/horizon/26.3 0755 root root -" ];
      virtualisation.memorySize = 1024;
    };

  testScript = ''
    machine.wait_for_unit("multi-user.target")
    machine.succeed("horizon-server --version")

    # The first start writes server.toml from `init`, then `config set` applies `port` and `settings`.
    machine.wait_until_succeeds("test -f /var/lib/horizon-server/server.toml")
    toml = machine.succeed("cat /var/lib/horizon-server/server.toml")
    for expected in ['port = 25570', 'name = "test"', 'seed = "12345"', 'difficulty = "hard"',
                     'max_players = 7', 'online_mode = false', 'resources = "/srv/horizon/26.3"']:
        assert expected in toml, f"{expected!r} not in server.toml:\n{toml}"
    machine.succeed("grep -qx 'eula=true' /var/lib/horizon-server/eula.txt")

    # No game files: the server names what is missing and where it looked, and never fetches them.
    machine.wait_until_succeeds("journalctl -u horizon-server | grep -q 'prepared game files'")

    # The next start puts back what the module manages and keeps the owner's other edits.
    machine.succeed("systemctl stop horizon-server")
    machine.succeed(
        "runuser -u horizon-server -- sed -i "
        "-e 's/^motd = .*/motd = \"kept\"/' -e 's/^difficulty = .*/difficulty = \"easy\"/' "
        "/var/lib/horizon-server/server.toml"
    )
    # Returns once ExecStartPre (init, config set) has run.
    machine.succeed("systemctl start horizon-server")
    toml = machine.succeed("cat /var/lib/horizon-server/server.toml")
    assert 'motd = "kept"' in toml, toml
    assert 'difficulty = "hard"' in toml, toml

    machine.succeed("nft list ruleset 2>/dev/null | grep -q 25570 || iptables -S | grep -q 25570")
    # Owned by the service's user and closed to others.
    machine.succeed("test \"$(stat -c %U:%a /var/lib/horizon-server)\" = horizon-server:700")
  '';
}
