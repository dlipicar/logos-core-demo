#!/usr/bin/env bash
# Runs a Logos blockchain node (a devnet follower) in a logoscore daemon and
# exports it, so Logos Core Demo apps can link to it.
#
#   scripts/node-daemon.sh start     build, configure, start the daemon and the node
#   scripts/node-daemon.sh invite    print a single-use invite for another machine
#   scripts/node-daemon.sh status    the daemon, its peers and the node's chain state
#   scripts/node-daemon.sh stop
#
# Environment:
#   CONFIG_DIR   the daemon's session dir (default ~/.logosctl, where the app
#                looks for this machine's local invite)
#   HOST         control address: 127.0.0.1 (this machine only, the default) or
#                0.0.0.0 to accept other machines
#   ADVERTISE    the address other machines dial (default: the daemon's guess)
#   PORT         control port (default 7443); keep it fixed, peers remember it
#   LOGOSCTL, NODE_MODULE   prebuilt logosctl and blockchain_module install dir
#                (default: built with nix from the pins below)
set -euo pipefail

here=$(cd "$(dirname "$0")/.." && pwd)
CONFIG_DIR=${CONFIG_DIR:-$HOME/.logosctl}
HOST=${HOST:-127.0.0.1}
PORT=${PORT:-7443}
# Sockets live under TMPDIR, and their paths hold ~104 bytes.
export TMPDIR=${NODE_TMPDIR:-/tmp/lcd-node-$(id -u)}
mkdir -p "$TMPDIR"

# The node module built with transport qt_remote_plain (exportable), against the
# node release that joins today's devnet.
MODULE_FLAKE=${MODULE_FLAKE:-github:logos-blockchain/logos-blockchain-module/feat/peering}
NODE_RELEASE=${NODE_RELEASE:-github:logos-blockchain/logos-blockchain/0.3.0-rc.4}

build() {
  if [ -z "${LOGOSCTL:-}" ]; then
    LOGOSCTL=$(nix build --no-link --print-out-paths "$here#daemon")/bin/logosctl
  fi
  if [ -z "${NODE_MODULE:-}" ]; then
    NODE_MODULE=$(nix build --no-link --print-out-paths "$MODULE_FLAKE#install" \
      --override-input logos-blockchain "$NODE_RELEASE")
  fi
}

ctl() { "$LOGOSCTL" --config-dir "$CONFIG_DIR" "$@"; }
call() { ctl call blockchain_module "$@"; }

start() {
  build
  mkdir -p "$CONFIG_DIR/node-modules"
  rm -rf "$CONFIG_DIR/node-modules/blockchain_module"
  cp -r "$NODE_MODULE/modules/blockchain_module" "$CONFIG_DIR/node-modules/"
  chmod -R u+w "$CONFIG_DIR/node-modules"
  local advertise=""
  [ -n "${ADVERTISE:-}" ] && advertise=", \"advertise\": \"$ADVERTISE\""
  cat > "$CONFIG_DIR/node-daemon.json" <<JSON
{
  "dirs": {"modules": "$CONFIG_DIR/node-modules"},
  "peering": {
    "name": "${NODE_NAME:-Logos node on $(hostname -s)}",
    "control": {"enabled": true, "host": "$HOST", "port": $PORT$advertise,
                "local_invite": {"allow": ["blockchain_module"]}},
    "exports": {"enabled": true, "modules": {"blockchain_module": {"events": true}}}
  }
}
JSON
  ctl daemon config set "$CONFIG_DIR/node-daemon.json" >/dev/null
  ctl daemon start --detach
  ctl module load blockchain_module >/dev/null

  # A devnet follower: stays in Bootstrapping, so it never proves or leads.
  local cfg
  cfg=$(call generate_user_config "@$here/config/devnet-user-config.json" | jq -r '.result.value // .result')
  if [ -z "$cfg" ] || [ "$cfg" = null ]; then
    echo "generate_user_config gave no path (a second run keeps the first config)" >&2
    cfg=$(find "$CONFIG_DIR" -name user_config.yaml -path '*blockchain_module*' | head -1)
  fi
  call merge_user_config "$cfg" "$cfg" "@$here/config/follower-mode.yaml" false false >/dev/null
  call start "$cfg" "" >/dev/null || true
  echo "node started with $cfg"
  echo "local invite for an app on this machine: $CONFIG_DIR/peering/local-invite"
}

case "${1:-}" in
  start) start ;;
  invite) build; ctl peer invite --ttl "${TTL:-3600}" --allow blockchain_module ;;
  status)
    build
    ctl peer status
    ctl peer ls
    call get_cryptarchia_info
    call get_network_info ;;
  stop) build; ctl daemon stop ;;
  *) sed -n '2,20p' "$0"; exit 1 ;;
esac
