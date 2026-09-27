# Logos Core Demo

Qt-free apps for macOS, Linux, Windows, Android and iOS that run a Logos runtime of their own
and use a Logos blockchain node that runs somewhere else: in a `logosctl` daemon
on the same computer, or on another one on the network.

It is [logoslib-android-poc](https://github.com/fryorcraken/logoslib-android-poc)
(v0.1.0) redone on the peering stack:

| | the POC | this demo |
|---|---|---|
| Platforms | Android | macOS, Linux, Windows, Android, iOS (simulator) |
| UI and runtime | Kotlin, JNI, Qt | Rust and Slint; the runtime is spawned (inside the app on iOS), no Qt anywhere |
| The node | runs on the phone | runs in a daemon; the app imports it through peering |
| Calling modules | `LogosAPI` by name | clients generated from each module's contract |

## What is in here

| Path | What |
|---|---|
| `app/` | The Slint app. The same crate is the desktop binary, the NativeActivity library on Android (`app/src/android.rs`), and the iOS app (`app/src/ios.rs`). |
| `demo-core/` | What the screens do: start the runtime, link a daemon, import the node, read it, watch blocks. Generated clients are in `src/clients/`. |
| `headless/` | A CLI over `demo-core`, for tests and scripts (JSON lines). |
| `cpp/bc-watch/` | The node, read from C++ through clients from `logos_generate_clients`. |
| `modules/` | `hello_module` (ping and an event), `bc_probe` (reads the node from a local module) and `fake_blockchain` (the node's shape, for tests). |
| `scripts/node-daemon.sh` | Runs a devnet follower in a `logosctl` daemon and exports it. |
| `nix/android.nix` | The Android library, the APK and an emulator SDK. |
| `nix/windows.nix` | The Windows programs and a portable zip, cross-built with MinGW. |
| `nix/ios.nix` | The runtime's C++ stack for the iOS simulator, the app and `LogosCoreDemo.app`. |

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
nix build .#bundle                    # a directory that runs without Nix: bin/logos-core-demo
nix build .#app-bundle                # macOS: Logos Core Demo.app, ad hoc signed
```

The bundle holds both apps and the runtime's executables in `bin/`, their
libraries in `lib/`, and the bundled and demo modules in `modules/` and
`app-modules/`. Copy it anywhere. On Linux the app takes X11/Wayland and GL
from the system.

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
`logos-pair:` link, running or not, so scanning a QR code of it with the camera
fills it in
(`scripts/node-daemon.sh invite | jq -r .invite | qrencode -t ansiutf8`). From the
emulator the host computer is `10.0.2.2`:

```bash
adb shell am start -a android.intent.action.VIEW -d "logos-pair:…@10.0.2.2:7443" co.logos.coredemo
```

`packages.aarch64-android.*` build on x86_64-linux, which logos-nix names as
Android's build system.

### Windows

```bash
nix build .#packages.x86_64-windows.zip    # on x86_64-linux: logos-core-demo-x86_64-windows.zip
```

Unzip it anywhere and run `bin\logos-core-demo.exe`. `bin\` holds the app, the
headless client, `logos_runtime.exe`, both module hosts and every DLL they or the
modules import (`nix/windows-dlls.py` walks the import tables at build time and
fails on one it cannot place); `modules\` and `app-modules\` hold the modules. The
app keeps its data in `%LOCALAPPDATA%\Logos Core Demo` and looks for a local
daemon's invite in `%LOCALAPPDATA%\.logosctl`, where `logosctl` keeps it.

Modules run in their own processes there: the in-process checkbox is hidden,
since the Windows runtime has no in-process facades yet.

### iOS

```bash
nix run .#ios-sim                     # on aarch64-darwin: install and run it in the booted simulator
nix build .#packages.aarch64-ios-simulator.app
```

iOS lets an app start no process, so the runtime runs inside the app. Every
module, the import's facade too, runs in the app's process, and the app binds no
local socket. `nix/ios.nix` builds the runtime's C++ stack with Xcode's clang
(Xcode 26.5, which the build checks), the app for `aarch64-apple-ios-sim`, and
`LogosCoreDemo.app`: the libraries in `Frameworks/`, the modules in `modules/`
and `app-modules/`, and the headless client beside the app. The simulator only,
for now.

The runtime starts with the app, so there is no Start or Stop. The simulator
shares the Mac's network: pair with a daemon on the Mac by code at `127.0.0.1`,
or paste an invite whose host is `127.0.0.1`. Launched with one, the app redeems
it at once:

```bash
SIMCTL_CHILD_LOGOS_CORE_DEMO_INVITE="$(cat invite.txt)" nix run .#ios-sim
```

The headless client runs in the simulator too:

```bash
app=$(nix build --print-out-paths .#packages.aarch64-ios-simulator.app)/LogosCoreDemo.app
xcrun simctl spawn booted $app/logos-core-demo-headless --embedded --runtime $app \
  --modules $app/app-modules --data /tmp/lcd-ios/data --tmp /tmp/lcd-ios/tmp \
  --invite invite.txt --hello --probe --watch 60
```

## Tests

```bash
nix flake check
```

- `e2e`: two runtimes on one machine. A daemon exports `fake_blockchain`; the
  headless app links it, imports it, reads it, probes it through `bc_probe`, and
  gets `newBlock` events. It survives the daemon's restart. It is refused when
  its consumer is not an allowed caller and when the peer grants nothing, and
  its live session ends when the rule narrows.
- `e2e-single-process`: the same, with every module (the import too) in the
  runtime's process.
- `e2e-embedded`: the same, with the runtime inside the app as on iOS. The app
  has no child process and binds no local socket.
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
