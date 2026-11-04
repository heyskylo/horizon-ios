#!/bin/sh
# Starts horizon-server in the server folder of a hosting panel (the Pterodactyl egg; docs/hosting.md).
# The program sits in ./horizon/, the prepared game files in ./resources/. The first start writes server.toml
# from the panel's HORIZON_* variables (horizon-server init: HORIZON_EULA, WORLD, SEED, WHITELIST, PLAYERS, OP,
# ONLINE); the port always follows the panel's allocation (SERVER_PORT).
cd "$(dirname "$0")" || exit 1
if [ ! -f server.toml ]; then
  ./horizon/horizon-server init --yes --port "${SERVER_PORT:-25565}" || exit 1
fi
sed -i "s/^port = .*/port = ${SERVER_PORT:-25565}/" server.toml
exec ./horizon/horizon-server run
