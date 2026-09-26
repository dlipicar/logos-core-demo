# Logos Core Demo

Qt-free apps for macOS, Linux and Android that run a Logos runtime of their own
and use a Logos blockchain node that runs somewhere else: in a `logosctl` daemon
on the same computer, or on another one on the network.

It is [logoslib-android-poc](https://github.com/fryorcraken/logoslib-android-poc)
(v0.1.0) redone on the peering stack:

| | the POC | this demo |
|---|---|---|
| Platforms | Android | macOS, Linux, Android |
| UI and runtime | Kotlin, JNI, Qt | Rust and Slint; the runtime is spawned, no Qt anywhere |
| The node | runs on the phone | runs in a daemon; the app imports it through peering |
| Calling modules | `LogosAPI` by name | clients generated from each module's contract |

## What is in here

| Path | What |
|---|---|
| `app/` | The Slint app. The same crate is the desktop binary and, on Android, the NativeActivity library (`app/src/android.rs`). |
| `demo-core/` | What the screens do: start the runtime, link a daemon, import the node, read it, watch blocks. Generated clients are in `src/clients/`. |
| `headless/` | A CLI over `demo-core`, for tests and scripts (JSON lines). |
| `cpp/bc-watch/` | The node, read from C++ through clients from `logos_generate_clients`. |
| `modules/` | `hello_module` (ping and an event), `bc_probe` (reads the node from a local module) and `fake_blockchain` (the node's shape, for tests). |
| `scripts/node-daemon.sh` | Runs a devnet follower in a `logosctl` daemon and exports it. |
| `nix/android.nix` | The Android library, the APK and an emulator SDK. |

## The node

```bash
scripts/node-daemon.sh start      # this computer only (127.0.0.1:7443)
HOST=0.0.0.0 ADVERTISE=192.168.1.5 scripts/node-daemon.sh start   # reachable on the LAN
scripts/node-daemon.sh invite     # a single-use invite for another computer
scripts/node-daemon.sh status
```

It builds `logosctl` and `blockchain_module` (from logos-blockchain-module's
`feat/peering` branch, against node 0.3.0-rc.4, which joins today's devnet). It
starts the daemon, configures a follower (it never proposes blocks) and exports
the node. The daemon's local invite grants `blockchain_module` to an app on the
same computer, and `invite` mints one that grants the same to another computer.

**Exposure.** Peering grants per module: a paired app may call every
`blockchain_module` method, `stop`, `purge_state` and the wallet ones included.
Only pair apps you trust.

## The apps

```bash
nix run .#app                         # macOS or Linux
nix run .#headless -- --help
nix run .#bc-watch -- --help
```

**Runtime** starts the app's own runtime (capability, modules_state, peering, and
the demo's modules). **Connect** links the daemon on this computer through its
local invite (`~/.logosctl/peering/local-invite`, or `$LOGOSCTL_CONFIG_DIR`), or
another computer by invite or by pairing code. **Node** imports `blockchain_module`
and shows the chain, the network, each new block, and the height as `bc_probe`
reads it through the import.

### Android

```bash
nix build .#packages.aarch64-android.apk
adb install -r result
```

The APK carries `logos_runtime`, the module hosts, every plugin and library as
`lib*.so` files (the only files Android extracts and lets an app execute), and
the module directories as assets. The app lays those out in its storage when it
starts.

Link a daemon by pairing code, or with an invite. An invite opens the app as a
`logos-pair:` link, so scanning a QR code of it with the camera fills it in
(`scripts/node-daemon.sh invite | jq -r .invite | qrencode -t ansiutf8`). From the
emulator the host computer is `10.0.2.2`:

```bash
adb shell am start -a android.intent.action.VIEW -d "logos-pair:…@10.0.2.2:7443" co.logos.coredemo
```

`packages.aarch64-android.*` build on x86_64-linux, which logos-nix names as
Android's build system.

## Tests

```bash
nix flake check
```

- `e2e`: two runtimes on one machine. A daemon exports `fake_blockchain`; the
  headless app links it, imports it, reads it, probes it through `bc_probe`, and
  gets `newBlock` events. It survives the daemon's restart. It is refused when
  its consumer is not an allowed caller and when the peer grants nothing, and
  its live session ends when the rule narrows.
- `clients-up-to-date`, `contracts-up-to-date`: the committed clients match the
  contracts, and the vendored `peering_module` contract matches the published one.

Regenerate the clients with `nix build .#clients` and copy `result/` into
`demo-core/src/clients`.

## Troubleshooting

- **A Mac cannot pair across the LAN** (`UNREACHABLE: connection timed out`,
  while `nc` reaches the port): an outgoing firewall such as Little Snitch holds
  new connections from unapproved binaries, and each nix build is a new one.
  Allow `logos_host_plain` and `logos_host_remote`, or pair over loopback.
- **Sockets**: the runtime's sockets live under a short `TMPDIR` (`/tmp/lcd-$USER`):
  a socket path holds about 104 bytes.
