//! The Slint front end. Every demo call blocks, so a worker thread runs them in
//! order and pushes what changed into the window.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

use demo_core::model::BlockEvent;
use demo_core::{exe, Demo, DemoEvent, EventSink, Paths, PendingPairing, Placement};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel, Weak};

slint::include_modules!();

#[cfg(target_os = "android")]
mod android;
#[cfg(target_os = "ios")]
pub mod ios;

const SHELL: &str = "core_demo";
// The Node tab's index in app.slint.
const NODE_TAB: i32 = 4;

enum Cmd {
    Start(Placement),
    Stop,
    LoadHello,
    Ping,
    Fire,
    Methods,
    LinkLocal(PathBuf),
    Redeem(String),
    Pair(String, i64),
    ConfirmPair,
    RefreshPeers,
    Import(String, String),
    RemoveImport,
    Probe,
    Event(DemoEvent),
}

/// Where the runtime and the demo's modules are, beside the installed app
/// (`../share/logos-core-demo`, or a .app's Resources), unless
/// LOGOS_CORE_DEMO_HOME says otherwise. A portable bundle keeps the runtime's
/// executables beside the app and its modules one level up.
pub fn desktop_paths() -> Paths {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_default();
    if std::env::var_os("LOGOS_CORE_DEMO_HOME").is_none() && exe_dir.join(exe("logos_runtime")).exists() {
        return Paths {
            runtime_bin: exe_dir.join(exe("logos_runtime")),
            host_plain_bin: exe_dir.join(exe("logos_host_plain")),
            host_remote_bin: exe_dir.join(exe("logos_host_remote")),
            bundled_modules: exe_dir.join("../modules"),
            app_modules: exe_dir.join("../app-modules"),
            data: data_dir(),
            tmp: short_tmp(),
        };
    }
    let home = std::env::var_os("LOGOS_CORE_DEMO_HOME").map(PathBuf::from).unwrap_or_else(|| {
        [exe_dir.join("../Resources"), exe_dir.join("../share/logos-core-demo")]
            .into_iter()
            .find(|dir| dir.join("runtime").exists())
            .unwrap_or_else(|| exe_dir.join("../share/logos-core-demo"))
    });
    Paths::desktop(&home.join("runtime"), &home.join("modules"), &data_dir(), &short_tmp())
}

fn data_dir() -> PathBuf {
    if cfg!(windows) {
        return windows_local_app_data().join("Logos Core Demo");
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    if cfg!(target_os = "macos") {
        home.join("Library/Application Support/Logos Core Demo")
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local/share"))
            .join("logos-core-demo")
    }
}

// Socket paths hold ~104 bytes, and macOS's own TMPDIR alone takes half of it.
// Windows endpoints are named pipes, not files.
fn short_tmp() -> PathBuf {
    if cfg!(windows) {
        return std::env::temp_dir().join("logos-core-demo");
    }
    let user = std::env::var("USER").unwrap_or_else(|_| "user".into());
    PathBuf::from(format!("/tmp/lcd-{user}"))
}

// Where logosctl keeps its config on Windows: %LOCALAPPDATA%, else %USERPROFILE%.
fn windows_local_app_data() -> PathBuf {
    ["LOCALAPPDATA", "USERPROFILE"]
        .iter()
        .find_map(|var| std::env::var_os(var))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn default_local_invite() -> String {
    let dir = std::env::var_os("LOGOSCTL_CONFIG_DIR").map(PathBuf::from).unwrap_or_else(|| {
        let home = if cfg!(windows) {
            windows_local_app_data()
        } else {
            std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default()
        };
        home.join(".logosctl")
    });
    dir.join("peering").join("local-invite").to_string_lossy().into_owned()
}

fn peer_name() -> String {
    #[cfg(target_os = "android")]
    if let Some(model) = android::device_model() {
        return format!("Logos Core Demo on {model}");
    }
    // iOS gives an app no host name; the simulator names its device.
    if cfg!(target_os = "ios") {
        let device = std::env::var("SIMULATOR_DEVICE_NAME").unwrap_or_else(|_| "iOS".into());
        return format!("Logos Core Demo on {device}");
    }
    let host = std::env::var(if cfg!(windows) { "COMPUTERNAME" } else { "HOSTNAME" })
        .ok()
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok().map(|s| s.trim().to_string()))
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "this device".into());
    format!("Logos Core Demo on {host}")
}

pub fn run(paths: Paths, desktop: bool) -> Result<(), slint::PlatformError> {
    run_with(paths, desktop, None, None)
}

/// A source of invites opened while the app runs, polled on the UI thread.
pub type LinkSource = Box<dyn Fn() -> Option<String>>;

/// `invite`: one the app was opened with, ready to redeem once the runtime runs.
/// `links`: invites opened later (Android: a logos-pair: link or QR code).
pub fn run_with(paths: Paths, desktop: bool, invite: Option<String>, links: Option<LinkSource>)
    -> Result<(), slint::PlatformError> {
    run_app(paths, desktop, invite, links, false)
}

/// `redeem`: redeem `invite` once the runtime runs, rather than wait for the button.
fn run_app(paths: Paths, desktop: bool, invite: Option<String>, links: Option<LinkSource>, redeem: bool)
    -> Result<(), slint::PlatformError> {
    let ui = AppWindow::new()?;
    ui.set_desktop(desktop);
    ui.set_single_process_available(!cfg!(any(windows, target_os = "ios")));
    // iOS starts no process: the runtime runs in the app from launch to exit.
    ui.set_runtime_controls(!cfg!(target_os = "ios"));
    ui.set_paste_available(cfg!(target_os = "ios"));
    ui.set_local_invite_path(default_local_invite().into());
    if let Some(invite) = &invite {
        ui.set_invite_text(invite.as_str().into());
        ui.set_current_tab(2);
    }
    let link_timer = slint::Timer::default();
    if let Some(links) = links {
        let weak = ui.as_weak();
        link_timer.start(slint::TimerMode::Repeated, Duration::from_millis(500), move || {
            if let (Some(link), Some(ui)) = (links(), weak.upgrade()) {
                ui.set_invite_text(link.into());
                ui.set_current_tab(2);
            }
        });
    }

    let (tx, rx) = channel::<Cmd>();
    let weak = ui.as_weak();
    std::thread::spawn({
        let tx = tx.clone();
        move || Worker::new(paths, weak, tx).run(rx)
    });
    if cfg!(target_os = "ios") {
        let _ = tx.send(Cmd::Start(Placement::Embedded));
    }
    if let (true, Some(invite)) = (redeem, invite) {
        let _ = tx.send(Cmd::Redeem(invite));
    }

    let send = |tx: &Sender<Cmd>, cmd: Cmd| {
        let _ = tx.send(cmd);
    };
    ui.on_start_runtime({
        let (tx, weak) = (tx.clone(), ui.as_weak());
        move || {
            let single = weak.upgrade().is_some_and(|ui| ui.get_single_process());
            send(&tx, Cmd::Start(if single { Placement::SingleProcess } else { Placement::Subprocess }))
        }
    });
    ui.on_stop_runtime({ let tx = tx.clone(); move || send(&tx, Cmd::Stop) });
    ui.on_load_hello({ let tx = tx.clone(); move || send(&tx, Cmd::LoadHello) });
    ui.on_ping({ let tx = tx.clone(); move || send(&tx, Cmd::Ping) });
    ui.on_fire({ let tx = tx.clone(); move || send(&tx, Cmd::Fire) });
    ui.on_methods({ let tx = tx.clone(); move || send(&tx, Cmd::Methods) });
    ui.on_refresh_peers({ let tx = tx.clone(); move || send(&tx, Cmd::RefreshPeers) });
    ui.on_confirm_pair({ let tx = tx.clone(); move || send(&tx, Cmd::ConfirmPair) });
    ui.on_remove_import({ let tx = tx.clone(); move || send(&tx, Cmd::RemoveImport) });
    ui.on_probe_node({ let tx = tx.clone(); move || send(&tx, Cmd::Probe) });
    ui.on_link_local({
        let (tx, weak) = (tx.clone(), ui.as_weak());
        move || {
            let path = weak.upgrade().map(|ui| ui.get_local_invite_path().to_string()).unwrap_or_default();
            send(&tx, Cmd::LinkLocal(PathBuf::from(path)))
        }
    });
    ui.on_redeem_invite({
        let (tx, weak) = (tx.clone(), ui.as_weak());
        move || {
            let text = weak.upgrade().map(|ui| ui.get_invite_text().to_string()).unwrap_or_default();
            send(&tx, Cmd::Redeem(text))
        }
    });
    ui.on_pair({
        let (tx, weak) = (tx.clone(), ui.as_weak());
        move || {
            if let Some(ui) = weak.upgrade() {
                let port = ui.get_pair_port().parse().unwrap_or(7443);
                send(&tx, Cmd::Pair(ui.get_pair_host().to_string(), port))
            }
        }
    });
    #[cfg(target_os = "ios")]
    ui.on_paste_invite({
        let weak = ui.as_weak();
        move || {
            if let (Some(text), Some(ui)) = (ios::pasteboard_text(), weak.upgrade()) {
                ui.set_invite_text(text.into());
            }
        }
    });
    ui.on_import_node({
        let (tx, weak) = (tx.clone(), ui.as_weak());
        move |peer| {
            let module = weak.upgrade().map(|ui| ui.get_remote_module().to_string()).unwrap_or_default();
            send(&tx, Cmd::Import(peer.to_string(), module))
        }
    });

    ui.run()
}

struct Worker {
    paths: Paths,
    ui: Weak<AppWindow>,
    tx: Sender<Cmd>,
    demo: Option<Demo>,
    pending: Option<PendingPairing>,
    log: VecDeque<String>,
}

impl Worker {
    fn new(paths: Paths, ui: Weak<AppWindow>, tx: Sender<Cmd>) -> Worker {
        Worker { paths, ui, tx, demo: None, pending: None, log: VecDeque::new() }
    }

    fn ui(&self, f: impl FnOnce(&AppWindow) + Send + 'static) {
        let _ = self.ui.upgrade_in_event_loop(move |ui| f(&ui));
    }

    fn log(&mut self, line: impl Into<String>) {
        self.log.push_front(line.into());
        self.log.truncate(40);
        let text: Vec<String> = self.log.iter().cloned().collect();
        self.ui(move |ui| ui.set_log_text(text.join("\n").into()));
    }

    fn run(mut self, rx: Receiver<Cmd>) {
        loop {
            match rx.recv_timeout(Duration::from_secs(5)) {
                Ok(cmd) => self.handle(cmd),
                Err(RecvTimeoutError::Timeout) => self.refresh_runtime(),
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        if let Some(demo) = self.demo.take() {
            demo.stop();
        }
    }

    fn handle(&mut self, cmd: Cmd) {
        if let Cmd::Event(event) = cmd {
            return self.on_event(event);
        }
        self.ui(|ui| ui.set_busy(true));
        let result = self.execute(cmd);
        if let Err(error) = result {
            self.log(format!("error: {error}"));
        }
        self.ui(|ui| ui.set_busy(false));
    }

    fn demo(&self) -> Result<&Demo, String> {
        self.demo.as_ref().ok_or_else(|| "start the runtime first".to_string())
    }

    fn execute(&mut self, cmd: Cmd) -> Result<(), String> {
        match cmd {
            Cmd::Start(placement) => self.start(placement)?,
            Cmd::Stop => {
                if let Some(demo) = self.demo.take() {
                    demo.stop();
                }
                self.ui(|ui| {
                    ui.set_runtime_state("STOPPED".into());
                    ui.set_import_state("not imported".into());
                    ui.set_modules_text("".into());
                });
                self.log("runtime stopped");
            }
            Cmd::LoadHello => {
                self.demo()?.load_hello()?;
                self.log("hello_module loaded");
                self.refresh_runtime();
            }
            Cmd::Ping => {
                let pong = self.demo()?.ping()?;
                self.ui(move |ui| ui.set_ping_result(pong.into()));
            }
            Cmd::Fire => {
                let n = self.demo()?.fire(&format!("tag-{}", self.log.len() + 1))?;
                self.log(format!("fire -> {n}"));
            }
            Cmd::Methods => {
                let contract = self.demo()?.hello_contract()?;
                self.ui(move |ui| ui.set_methods_text(contract.into()));
            }
            Cmd::LinkLocal(path) => {
                let peer = self.demo()?.link_local_daemon(&path)?;
                self.connected(&peer)?;
            }
            Cmd::Redeem(text) => {
                let peer = self.demo()?.redeem_invite(&text)?;
                self.connected(&peer)?;
            }
            Cmd::Pair(host, port) => {
                let pending = self.demo()?.pair_with(&host, port)?;
                let code = pending.code.clone();
                self.log(format!("pairing code {code} with {}", pending.peer_display_id));
                self.pending = Some(pending);
                self.ui(move |ui| {
                    ui.set_pair_code(code.into());
                    ui.set_connect_status("Compare the code, then press Codes match".into());
                });
            }
            Cmd::ConfirmPair => {
                let pending = self.pending.take().ok_or("no pairing in progress")?;
                self.ui(|ui| ui.set_connect_status("Waiting for the other side to accept...".into()));
                let peer = self.demo()?.confirm_pairing(&pending, Duration::from_secs(180))?;
                self.ui(|ui| ui.set_pair_code("".into()));
                self.connected(&peer)?;
            }
            Cmd::RefreshPeers => self.refresh_peers()?,
            Cmd::Import(peer, module) => self.import(&peer, &module)?,
            Cmd::RemoveImport => {
                self.demo()?.remove_node_import()?;
                self.ui(|ui| ui.set_import_state("not imported".into()));
                self.log("node import removed");
            }
            Cmd::Probe => {
                let probe = self.demo()?.probe()?;
                let text = match probe.get("info").and_then(|i| i.get("height")) {
                    Some(height) if probe.get("ok").and_then(|v| v.as_bool()) == Some(true) => format!(
                        "height {height} in {} ms",
                        probe.get("latency_ms").and_then(|v| v.as_i64()).unwrap_or(0)
                    ),
                    _ => probe.get("error").map(|e| e.to_string()).unwrap_or_else(|| probe.to_string()),
                };
                self.ui(move |ui| ui.set_probe(text.into()));
            }
            Cmd::Event(_) => {}
        }
        Ok(())
    }

    fn start(&mut self, placement: Placement) -> Result<(), String> {
        if self.demo.is_some() {
            return Ok(());
        }
        self.ui(|ui| ui.set_runtime_state("STARTING".into()));
        let tx = self.tx.clone();
        let sink: EventSink = std::sync::Arc::new(move |event| {
            let _ = tx.send(Cmd::Event(event));
        });
        let demo = match Demo::start(SHELL, &peer_name(), &self.paths, placement, sink) {
            Ok(demo) => demo,
            Err(e) => {
                self.ui(|ui| ui.set_runtime_state("STOPPED".into()));
                return Err(e);
            }
        };
        let status = demo.peering_status().unwrap_or_default();
        let display = status.get("display_id").and_then(|v| v.as_str()).unwrap_or("-").to_string();
        self.demo = Some(demo);
        self.ui(move |ui| {
            ui.set_runtime_state("RUNNING".into());
            ui.set_display_id(display.into());
        });
        self.log("runtime started");
        self.refresh_runtime();
        self.refresh_peers()
    }

    fn connected(&mut self, peer: &str) -> Result<(), String> {
        self.log(format!("paired with {peer}"));
        self.refresh_peers()?;
        let module = self.ui.upgrade().map(|ui| ui.get_remote_module().to_string());
        let module = module.filter(|m| !m.is_empty()).unwrap_or_else(|| demo_core::NODE.to_string());
        self.import(peer, &module)
    }

    fn import(&mut self, peer: &str, module: &str) -> Result<(), String> {
        let demo = self.demo()?;
        demo.import_node(peer, module)?;
        self.ui(|ui| ui.set_import_state("connecting".into()));
        demo.wait_import_ready(demo_core::NODE, Duration::from_secs(60))?;
        demo.start_node_watch(Duration::from_secs(5))?;
        self.ui(|ui| {
            ui.set_import_state("ready".into());
            ui.set_connect_status("Linked; the node is on the Node tab".into());
            ui.set_current_tab(NODE_TAB);
        });
        self.log(format!("imported {module} as {}", demo_core::NODE));
        Ok(())
    }

    fn refresh_peers(&mut self) -> Result<(), String> {
        let peers = self.demo()?.peers()?;
        let rows: Vec<PeerRow> = peers
            .into_iter()
            .map(|p| PeerRow {
                runtime_id: p.runtime_id.into(),
                name: if p.display_name.is_empty() { p.alias.into() } else { p.display_name.into() },
                display_id: p.display_id.into(),
                status: p.status.into(),
            })
            .collect();
        self.ui(move |ui| ui.set_peers(ModelRc::new(VecModel::from(rows))));
        Ok(())
    }

    fn refresh_runtime(&mut self) {
        let Some(demo) = &self.demo else { return };
        let modules = demo.modules().unwrap_or_default();
        let text: Vec<String> = modules.iter().map(|(name, status)| format!("{name}  {status}")).collect();
        let stats = demo.module_stats().unwrap_or_default();
        let processes: Vec<String> = stats
            .as_array()
            .map(|rows| {
                rows.iter()
                    .map(|m| {
                        format!(
                            "{}  pid {}  cpu {:.1}%  {:.0} MB",
                            m.get("name").and_then(|v| v.as_str()).unwrap_or("?"),
                            m.get("pid").map(|v| v.to_string()).unwrap_or_else(|| "-".into()),
                            m.get("cpu_percent").or_else(|| m.get("cpu")).and_then(|v| v.as_f64()).unwrap_or(0.0),
                            m.get("memory_mb").or_else(|| m.get("memoryMB")).and_then(|v| v.as_f64()).unwrap_or(0.0)
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        self.ui(move |ui| {
            ui.set_modules_text(text.join("\n").into());
            ui.set_processes_text(processes.join("\n").into());
        });
    }

    fn on_event(&mut self, event: DemoEvent) {
        match event {
            DemoEvent::HelloFired { tag, count } => {
                self.ui(move |ui| ui.set_last_event(format!("{tag} (#{count})").into()));
            }
            DemoEvent::PeersChanged => {
                let _ = self.refresh_peers();
            }
            DemoEvent::PairingRequested(request) => self.log(format!("pairing requested: {request}")),
            DemoEvent::ImportState { name, state, reason } => {
                self.log(format!("{name}: {state} {reason}"));
                if name == demo_core::NODE {
                    let text = if reason.is_empty() { state } else { format!("{state} ({reason})") };
                    self.ui(move |ui| ui.set_import_state(text.into()));
                }
            }
            DemoEvent::Node(snapshot) => {
                let chain = snapshot.chain.clone();
                let lag = snapshot.lag();
                let slots = match (&chain, snapshot.current_slot) {
                    (Some(c), Some(now)) => format!("{} / now {} (lag {})", c.slot, now, lag.unwrap_or(0)),
                    (Some(c), None) => c.slot.to_string(),
                    _ => "-".into(),
                };
                let network = snapshot
                    .network
                    .map(|n| format!("{} peers, {} connections", n.peers, n.connections))
                    .unwrap_or_else(|| "-".into());
                let synced = if snapshot.at_tip() { "  (at the tip)" } else { "" };
                self.ui(move |ui| {
                    ui.set_chain_id(SharedString::from(snapshot.chain_id.clone().unwrap_or_else(|| "-".into())));
                    ui.set_chain_mode(chain.as_ref().map(|c| c.mode.clone()).unwrap_or_else(|| "-".into()).into());
                    ui.set_chain_height(chain.as_ref().map(|c| format!("{}{synced}", c.height)).unwrap_or_else(|| "-".into()).into());
                    ui.set_slots(slots.into());
                    ui.set_lib_slot(chain.as_ref().map(|c| c.lib_slot.to_string()).unwrap_or_else(|| "-".into()).into());
                    ui.set_network(network.into());
                    ui.set_node_error(snapshot.error.clone().unwrap_or_default().into());
                });
            }
            DemoEvent::NewBlock { count, event } => {
                let last = match event {
                    BlockEvent::Block { slot, parent } => format!(
                        "{}{}",
                        slot.map(|s| format!("slot {s}  ")).unwrap_or_default(),
                        parent.map(|p| format!("parent {}…", p.chars().take(12).collect::<String>())).unwrap_or_default()
                    ),
                    BlockEvent::StreamEnded => "stream ended, re-subscribing".into(),
                };
                self.ui(move |ui| {
                    ui.set_new_blocks(count.to_string().into());
                    ui.set_last_block(last.into());
                });
            }
            DemoEvent::RuntimeExited(reason) => {
                self.demo = None;
                self.log(format!("runtime exited: {reason}"));
                self.ui(|ui| ui.set_runtime_state("STOPPED".into()));
            }
        }
    }
}
