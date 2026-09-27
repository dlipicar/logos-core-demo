//! The demo's runtime, and what the screens do with it. Every method blocks;
//! call them off a UI thread.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use logos_rust_sdk::host::{Config, LoadDeps, LogosCore, Subscription};
use serde_json::{json, Value};

use crate::clients::bc_probe::BcProbeClient;
use crate::clients::blockchain_module::BlockchainModuleClient;
use crate::clients::hello_module::HelloModuleClient;
use crate::clients::peering_module::PeeringModuleClient;
use crate::invite::Invite;
use crate::model::{self, BlockEvent, ChainInfo, NetworkInfo, NodeSnapshot};

/// The name the node is imported under here; the peer may call it anything.
pub const NODE: &str = "blockchain_module";
const PROBE: &str = "bc_probe";
const HELLO: &str = "hello_module";

/// Where the runtime and the demo's modules live.
#[derive(Debug, Clone)]
pub struct Paths {
    pub runtime_bin: PathBuf,
    pub host_plain_bin: PathBuf,
    pub host_remote_bin: PathBuf,
    pub bundled_modules: PathBuf,
    pub app_modules: PathBuf,
    pub data: PathBuf,
    pub tmp: PathBuf,
}

impl Paths {
    /// The desktop layout: `<runtime>/bin`, `<runtime>/modules` (bundled).
    pub fn desktop(runtime: &Path, app_modules: &Path, data: &Path, tmp: &Path) -> Paths {
        Paths {
            runtime_bin: runtime.join("bin/logos_runtime"),
            host_plain_bin: runtime.join("bin/logos_host_plain"),
            host_remote_bin: runtime.join("bin/logos_host_remote"),
            bundled_modules: runtime.join("modules"),
            app_modules: app_modules.to_path_buf(),
            data: data.to_path_buf(),
            tmp: tmp.to_path_buf(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum DemoEvent {
    HelloFired { tag: String, count: i64 },
    PeersChanged,
    PairingRequested(Value),
    ImportState { name: String, state: String, reason: String },
    Node(NodeSnapshot),
    NewBlock { count: u64, event: BlockEvent },
    RuntimeExited(String),
}

pub type EventSink = Arc<dyn Fn(DemoEvent) + Send + Sync>;

#[derive(Debug, Clone, PartialEq)]
pub struct Peer {
    pub runtime_id: String,
    pub alias: String,
    pub display_name: String,
    pub display_id: String,
    pub status: String,
}

/// A pairing by code: compare `code` with the other screen, then confirm.
#[derive(Debug, Clone)]
pub struct PendingPairing {
    pub id: String,
    pub code: String,
    pub peer_display_id: String,
}

pub struct Demo {
    core: LogosCore,
    sink: EventSink,
    subscriptions: Mutex<Vec<Subscription>>,
    watcher: Mutex<Option<Watcher>>,
}

fn fail(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn map_value(map: BTreeMap<String, Value>) -> Value {
    Value::Object(map.into_iter().collect())
}

// peering_module answers every method with an object, and refusals as {error}.
fn peering(reply: Result<BTreeMap<String, Value>, logos_rust_sdk::LogosError>) -> Result<Value, String> {
    let value = map_value(reply.map_err(fail)?);
    match value.get("error").and_then(Value::as_str) {
        Some(error) => Err(error.to_string()),
        None => Ok(value),
    }
}

impl Demo {
    /// Spawns this app's runtime; up to two minutes while it loads its modules.
    /// `single_process` keeps every module in the runtime's process, the import too.
    pub fn start(shell: &str, name: &str, paths: &Paths, single_process: bool, sink: EventSink)
        -> Result<Demo, String> {
        std::fs::create_dir_all(&paths.data).map_err(fail)?;
        std::fs::create_dir_all(&paths.tmp).map_err(fail)?;
        // The app's own modules ship with it, so they are bundled too: only a
        // bundled module may run in the runtime's process.
        let config = Config::new(shell)
            .bundled_modules_dir(&paths.bundled_modules)
            .bundled_modules_dir(&paths.app_modules)
            .persistence(paths.data.join("runtime"))
            .peering(json!({ "name": name }))
            .runtime_path(&paths.runtime_bin)
            .host_plain_path(&paths.host_plain_bin)
            .host_remote_path(&paths.host_remote_bin)
            .tmp_dir(&paths.tmp);
        let config = if single_process {
            config.placement_policy(json!({ "single_process": true }))
        } else {
            config
        };
        let core = LogosCore::start(config).map_err(fail)?;
        let exit_sink = sink.clone();
        core.on_exit(move |reason| exit_sink(DemoEvent::RuntimeExited(reason)));
        let demo = Demo { core, sink, subscriptions: Mutex::new(Vec::new()), watcher: Mutex::new(None) };
        demo.follow_peering()?;
        Ok(demo)
    }

    pub fn core(&self) -> &LogosCore {
        &self.core
    }

    fn follow_peering(&self) -> Result<(), String> {
        let mut subs = self.subscriptions.lock().unwrap();
        let sink = self.sink.clone();
        subs.push(self.core.subscribe("peering_module", "peersChanged", move |_, _| sink(DemoEvent::PeersChanged))
            .map_err(fail)?);
        let sink = self.sink.clone();
        subs.push(self.core.subscribe("peering_module", "pairingRequested", move |_, data| {
            sink(DemoEvent::PairingRequested(data.get(0).cloned().unwrap_or(data)))
        }).map_err(fail)?);
        let sink = self.sink.clone();
        subs.push(self.core.subscribe("peering_module", "importStateChanged", move |_, data| {
            let text = |i: usize| data.get(i).and_then(Value::as_str).unwrap_or_default().to_string();
            sink(DemoEvent::ImportState { name: text(0), state: text(1), reason: text(2) })
        }).map_err(fail)?);
        Ok(())
    }

    /// `(name, status)` of every module the runtime knows.
    pub fn modules(&self) -> Result<Vec<(String, String)>, String> {
        let list = self.core.list_modules().map_err(fail)?;
        let mut rows: Vec<(String, String)> = list
            .as_array()
            .map(|rows| {
                rows.iter()
                    .map(|m| {
                        let text = |k: &str| m.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
                        (text("name"), text("status"))
                    })
                    .collect()
            })
            .unwrap_or_default();
        rows.sort();
        Ok(rows)
    }

    /// Per-module CPU and memory, as core_service reports them.
    pub fn module_stats(&self) -> Result<Value, String> {
        self.core.module_stats().map_err(fail)
    }

    // ── the local module ────────────────────────────────────────────────────

    pub fn load_hello(&self) -> Result<(), String> {
        self.core.load_module(HELLO, LoadDeps::Required).map_err(fail)?;
        let sink = self.sink.clone();
        let sub = self.core.subscribe(HELLO, "fired", move |_, data| {
            let tag = data.get(0).and_then(Value::as_str).unwrap_or_default().to_string();
            let count = data.get(1).and_then(Value::as_i64).unwrap_or(0);
            sink(DemoEvent::HelloFired { tag, count })
        }).map_err(fail)?;
        self.subscriptions.lock().unwrap().push(sub);
        Ok(())
    }

    pub fn ping(&self) -> Result<String, String> {
        HelloModuleClient::new().ping().map_err(fail)
    }

    pub fn fire(&self, tag: &str) -> Result<i64, String> {
        HelloModuleClient::new().fire(tag).map_err(fail)
    }

    /// The module's own contract, from the built-in `lidl()` method.
    pub fn hello_contract(&self) -> Result<String, String> {
        let reply = self.core.call(HELLO, "lidl", json!([]), Duration::from_secs(10)).map_err(fail)?;
        Ok(reply.as_str().map(str::to_string).unwrap_or_else(|| reply.to_string()))
    }

    // ── peering ─────────────────────────────────────────────────────────────

    pub fn peering_status(&self) -> Result<Value, String> {
        peering(PeeringModuleClient::new().status())
    }

    pub fn peers(&self) -> Result<Vec<Peer>, String> {
        let reply = peering(PeeringModuleClient::new().peers())?;
        Ok(reply
            .get("peers")
            .and_then(Value::as_array)
            .map(|peers| {
                peers.iter()
                    .map(|p| {
                        let text = |k: &str| p.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
                        Peer {
                            runtime_id: text("runtime_id"),
                            alias: text("alias"),
                            display_name: text("display_name"),
                            display_id: text("display_id"),
                            status: text("status"),
                        }
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    /// Redeems the local invite a daemon on this machine keeps for its own user.
    pub fn link_local_daemon(&self, invite_file: &Path) -> Result<String, String> {
        let text = std::fs::read_to_string(invite_file)
            .map_err(|e| format!("{}: {e}", invite_file.display()))?;
        self.redeem_invite(text.lines().next().unwrap_or_default())
    }

    /// Pairs with the runtime that minted `invite`; its runtime id once paired.
    pub fn redeem_invite(&self, invite: &str) -> Result<String, String> {
        let parsed = Invite::parse(invite.trim())?;
        if self.peers()?.iter().any(|p| p.runtime_id == parsed.runtime_id) {
            return Ok(parsed.runtime_id);
        }
        peering(PeeringModuleClient::new().redeem_invite(invite.trim(), None))?;
        self.wait_for_peer(&parsed.runtime_id, Duration::from_secs(30))?;
        Ok(parsed.runtime_id)
    }

    /// Starts a code pairing with a runtime whose pairing window is open.
    pub fn pair_with(&self, host: &str, port: i64) -> Result<PendingPairing, String> {
        let reply = peering(PeeringModuleClient::new().pair_with(host, port))?;
        let text = |k: &str| reply.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
        Ok(PendingPairing { id: text("id"), code: text("code"), peer_display_id: text("peer_display_id") })
    }

    /// After the codes matched: confirms here, then waits for the other side.
    pub fn confirm_pairing(&self, pairing: &PendingPairing, timeout: Duration) -> Result<String, String> {
        let before: Vec<String> = self.peers()?.into_iter().map(|p| p.runtime_id).collect();
        peering(PeeringModuleClient::new().confirm_pairing(&pairing.id, None))?;
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if let Some(peer) = self.peers()?.into_iter().find(|p| !before.contains(&p.runtime_id)) {
                return Ok(peer.runtime_id);
            }
            if let Some(entry) = self.pending_entry(&pairing.id)? {
                if entry.get("state").and_then(Value::as_str) == Some("failed") {
                    let why = entry.get("error").and_then(Value::as_str).unwrap_or("the other side refused");
                    return Err(format!("pairing failed: {why}"));
                }
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        Err("the other side did not accept the pairing in time".into())
    }

    fn pending_entry(&self, id: &str) -> Result<Option<Value>, String> {
        let reply = peering(PeeringModuleClient::new().pending())?;
        Ok(reply
            .get("pending")
            .and_then(Value::as_array)
            .and_then(|list| list.iter().find(|p| p.get("id").and_then(Value::as_str) == Some(id)).cloned()))
    }

    fn wait_for_peer(&self, runtime_id: &str, timeout: Duration) -> Result<(), String> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if self.peers()?.iter().any(|p| p.runtime_id == runtime_id) {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        Err("the pairing did not complete in time".into())
    }

    /// What `peer` shares with this runtime: `{module: {events, loaded}}`.
    pub fn peer_exports(&self, peer: &str) -> Result<Value, String> {
        let reply = peering(PeeringModuleClient::new().peer_exports(peer))?;
        Ok(reply.get("exports").cloned().unwrap_or(Value::Null))
    }

    pub fn remove_peer(&self, peer: &str) -> Result<(), String> {
        peering(PeeringModuleClient::new().remove_peer(peer)).map(|_| ())
    }

    /// Imports the peer's node module as this runtime's `blockchain_module`,
    /// callable by this app and by bc_probe, with its events.
    pub fn import_node(&self, peer: &str, remote_module: &str) -> Result<(), String> {
        self.import_node_for(peer, remote_module, &[self.core.shell_name(), PROBE])
    }

    /// The same import, callable only by `callers` of this runtime.
    pub fn import_node_for(&self, peer: &str, remote_module: &str, callers: &[&str]) -> Result<(), String> {
        let config: BTreeMap<String, Value> = [
            ("from", json!(peer)),
            ("module", json!(remote_module)),
            ("allowed_callers", json!(callers)),
            ("events", json!(true)),
            ("prefer", json!("remote")),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
        peering(PeeringModuleClient::new().set_import(NODE, &config)).map(|_| ())
    }

    /// Removes the import; bc_probe goes first, since removal does not cascade.
    pub fn remove_node_import(&self) -> Result<(), String> {
        self.stop_node_watch();
        let _ = self.core.unload_module(PROBE);
        peering(PeeringModuleClient::new().remove_import(NODE)).map(|_| ())
    }

    /// `(state, reason)` of an import: configured, connecting, ready or error.
    pub fn import_state(&self, name: &str) -> Result<Option<(String, String)>, String> {
        let states = PeeringModuleClient::new().import_states().map_err(fail)?;
        Ok(states.get(name).map(|s| {
            let text = |k: &str| s.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
            (text("state"), text("reason"))
        }))
    }

    pub fn wait_import_ready(&self, name: &str, timeout: Duration) -> Result<(), String> {
        let deadline = Instant::now() + timeout;
        let mut last = None;
        while Instant::now() < deadline {
            match self.import_state(name)? {
                Some((state, _)) if state == "ready" => return Ok(()),
                other => last = other,
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        Err(match last {
            Some((state, reason)) => format!("{name} is {state}: {reason}"),
            None => format!("{name} is not imported"),
        })
    }

    // ── the node ────────────────────────────────────────────────────────────

    /// One reading of the node, through the typed client.
    pub fn read_node(&self, chain_id: Option<String>) -> NodeSnapshot {
        Demo::read_node_with(chain_id)
    }

    /// Polls the node every `every` and follows its newBlock events.
    pub fn start_node_watch(&self, every: Duration) -> Result<(), String> {
        self.stop_node_watch();
        let blocks = Arc::new(AtomicU64::new(0));
        let sink = self.sink.clone();
        let count = blocks.clone();
        let sub = self.core.subscribe(NODE, "newBlock", move |_, data| {
            let text = data.get(0).and_then(Value::as_str).unwrap_or("null").to_string();
            let event = model::block_event(&text);
            if event == BlockEvent::StreamEnded {
                // The node's stream ended; ask for a new one.
                std::thread::spawn(|| {
                    let _ = BlockchainModuleClient::new().subscribe_to_new_blocks();
                });
            } else {
                count.fetch_add(1, Ordering::Relaxed);
            }
            sink(DemoEvent::NewBlock { count: count.load(Ordering::Relaxed), event })
        }).map_err(fail)?;

        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let sink = self.sink.clone();
        let demo_core = self.core.clone();
        let thread = std::thread::spawn(move || {
            let _core = demo_core;
            let mut chain_id = None;
            while !flag.load(Ordering::Relaxed) {
                let snapshot = Demo::read_node_with(chain_id.clone());
                chain_id = snapshot.chain_id.clone();
                sink(DemoEvent::Node(snapshot));
                let slept = Instant::now();
                while slept.elapsed() < every && !flag.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        });
        *self.watcher.lock().unwrap() = Some(Watcher { stop, thread: Some(thread), _subscription: sub });
        Ok(())
    }

    fn read_node_with(chain_id: Option<String>) -> NodeSnapshot {
        let node = BlockchainModuleClient::new();
        let read = |reply: Result<Value, logos_rust_sdk::LogosError>| -> Result<Value, String> {
            model::result_value(&reply.map_err(fail)?)
        };
        let mut snapshot = NodeSnapshot { chain_id, ..NodeSnapshot::default() };
        match read(node.get_cryptarchia_info()) {
            Ok(info) => snapshot.chain = ChainInfo::parse(&info),
            Err(e) => snapshot.error = Some(e),
        }
        if let Ok(info) = read(node.get_network_info()) {
            snapshot.network = NetworkInfo::parse(&info);
        }
        if let Ok(info) = read(node.get_time_info()) {
            snapshot.current_slot = model::current_slot(&info);
        }
        if snapshot.chain_id.is_none() {
            snapshot.chain_id = read(node.get_chain_id()).ok().map(|v| match v {
                Value::String(s) => s,
                other => other.to_string(),
            });
        }
        snapshot
    }

    pub fn stop_node_watch(&self) {
        if let Some(mut watcher) = self.watcher.lock().unwrap().take() {
            watcher.stop();
        }
    }

    /// bc_probe's reading of the node: a module of this runtime calling the
    /// imported one. Loads bc_probe on first use, once the import is ready.
    pub fn probe(&self) -> Result<Value, String> {
        self.core.load_module(PROBE, LoadDeps::Required).map_err(fail)?;
        Ok(map_value(BcProbeClient::new().chain_info_via_bc().map_err(fail)?))
    }

    /// Stops the runtime, and every module and import with it.
    pub fn stop(self) {
        self.stop_node_watch();
        self.subscriptions.lock().unwrap().clear();
    }
}

impl Drop for Demo {
    fn drop(&mut self) {
        self.stop_node_watch();
    }
}

struct Watcher {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    _subscription: Subscription,
}

impl Watcher {
    fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
