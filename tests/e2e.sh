#!/usr/bin/env bash
# Two runtimes on one machine: a daemon exports fake_blockchain, and the
# headless app links it through the local invite, imports it as its
# blockchain_module, and reads it directly, through bc_probe and by events.
#   CTL=<logosctl> HEADLESS=<logos-core-demo-headless> FAKE=<fake_blockchain install> tests/e2e.sh
set -euo pipefail

work=$(mktemp -d /tmp/lcd-e2e.XXXXXX)   # short: socket paths hold ~104 bytes
trap 'cleanup' EXIT
daemon() { TMPDIR=$work/tb "$CTL" --config-dir "$work/b" "$@"; }
cleanup() {
  daemon daemon stop >/dev/null 2>&1 || true
  [ -n "${KEEP:-}" ] || rm -rf "$work"
}
mkdir -p "$work/b" "$work/b-modules" "$work/tb" "$work/ta" "$work/a-data"
cp -r "$FAKE"/modules/. "$work/b-modules/"
chmod -R u+w "$work/b-modules"

# A fixed port: a peer keeps the port it paired with, so an ephemeral one would
# not survive the daemon's restart.
port=$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1])')
cat > "$work/b.json" <<JSON
{
  "dirs": {"modules": "$work/b-modules"},
  "peering": {
    "name": "node-box",
    "control": {"enabled": true, "host": "127.0.0.1", "port": $port,
                "local_invite": {"path": "$work/b-invite", "allow": ["fake_blockchain"]}},
    "exports": {"enabled": true, "modules": {"fake_blockchain": {"events": true}}}
  }
}
JSON
daemon daemon config set "$work/b.json" >/dev/null
daemon daemon start --detach >/dev/null
daemon module load fake_blockchain >/dev/null
for _ in $(seq 50); do [ -s "$work/b-invite" ] && break; sleep 0.2; done

# One app instance per scenario, each with an identity of its own.
app() {
  local name=$1; shift
  mkdir -p "$work/$name-tmp"
  "$HEADLESS" --runtime "$APP_HOME/runtime" --modules "$APP_HOME/modules" \
    --data "$work/$name-data" --tmp "$work/$name-tmp" --name "$name" \
    --remote-module fake_blockchain ${SINGLE_PROCESS:+--single-process} "$@"
}
wait_invite() {  # the local invite is replaced once used
  for _ in $(seq 100); do [ -s "$work/b-invite" ] && [ "$(cat "$work/b-invite")" != "${1:-}" ] && return; sleep 0.2; done
  echo "no fresh local invite" >&2; return 1
}
step() { jq -c "select(.step == \"$2\") | .result" "$1"; }

echo "== link, import, read, probe, caller gate, events"
used=$(cat "$work/b-invite")
out="$work/a1.jsonl"
app a1 --hello --link-local "$work/b-invite" --caller-gate --narrow-live --probe --whoami --watch 15 --expect-blocks 3 | tee "$out"
[ "$(step "$out" hello | jq -r .ping)" = pong ]
[ "$(step "$out" whoami | jq -r .remote)" = true ]
[ "$(step "$out" whoami | jq -r .name)" = core_demo ]
[ "$(step "$out" node | jq -r .chain_id)" = fake-devnet ]
[ "$(step "$out" probe | jq -r .ok)" = true ]
# Where the import's facade runs: in the runtime's process, or a host of its own.
if [ -n "${SINGLE_PROCESS:-}" ]; then
  [ "$(step "$out" processes | jq length)" = 0 ]
else
  [ "$(step "$out" processes | jq 'index("blockchain_module") != null')" = true ]
fi
[ "$(step "$out" callers | jq -r .shell_refused)" = true ]
[ "$(step "$out" narrow_live | jq -r .after_refused)" = true ]
[ "$(step "$out" blocks | jq -r .new_block_events)" -ge 3 ]

echo "== the daemon restarts under a live import"
wait_invite "$used"
out="$work/a2.jsonl"
app a2 --link-local "$work/b-invite" --await-restart 120 > "$out" &
pid=$!
for _ in $(seq 300); do grep -q awaiting_restart "$out" 2>/dev/null && break; sleep 0.2; done
grep -q awaiting_restart "$out" || { echo "the app never linked"; cat "$out"; exit 1; }
echo "-- stopping the daemon"
daemon daemon stop >/dev/null
# Down until the app has seen the loss (the facade checks its peer every 15 s).
for _ in $(seq 300); do grep -q '"state":"error"' "$out" && break; sleep 0.2; done
grep -q '"state":"error"' "$out" || { echo "the app never saw the daemon go"; cat "$out"; exit 1; }
echo "-- restarting the daemon"
daemon daemon start --detach >/dev/null
daemon module load fake_blockchain >/dev/null
wait "$pid" || {
  echo "the app did not recover"; cat "$out"
  echo "-- the daemon's log"; tail -80 "$work"/b/logs/*.log 2>/dev/null
  exit 1
}
cat "$out"
[ "$(step "$out" restart | jq -r .events_after)" -ge 2 ]

echo "== paired by an invite that grants nothing"
daemon peer invite --ttl 600 --json | jq -r '.invite // .result.invite' > "$work/plain-invite"
out="$work/a3.jsonl"
app a3 --invite "$work/plain-invite" --expect-denied | tee "$out"
[ "$(step "$out" denied | jq -r .refused)" = true ]
echo "e2e: PASS"
