//! Runs the demo's steps without a UI and prints one JSON object per line.
//!
//!   logos-core-demo-headless --runtime DIR --modules DIR --data DIR --tmp DIR
//!       [--name NAME] [--hello]
//!       [--link-local FILE | --invite FILE | --pair HOST:PORT]
//!       [--remote-module NAME] [--watch SECONDS] [--expect-blocks N] [--probe]
//!       [--whoami] [--caller-gate] [--expect-denied] [--await-restart SECONDS]

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use demo_core::{Demo, DemoEvent, Paths};
use serde_json::{json, Value};

fn emit(step: &str, value: Value) {
    println!("{}", json!({ "step": step, "result": value }));
}

fn fail(step: &str, error: String) -> ! {
    println!("{}", json!({ "step": step, "error": error }));
    std::process::exit(1);
}

struct Args {
    runtime: PathBuf,
    modules: PathBuf,
    data: PathBuf,
    tmp: PathBuf,
    name: String,
    hello: bool,
    link_local: Option<PathBuf>,
    invite: Option<PathBuf>,
    pair: Option<(String, i64)>,
    remote_module: String,
    watch: u64,
    expect_blocks: u64,
    probe: bool,
    whoami: bool,
    caller_gate: bool,
    expect_denied: bool,
    await_restart: u64,
}

fn parse_args() -> Args {
    let mut args = Args {
        runtime: PathBuf::new(),
        modules: PathBuf::new(),
        data: PathBuf::new(),
        tmp: PathBuf::new(),
        name: "logos-core-demo".into(),
        hello: false,
        link_local: None,
        invite: None,
        pair: None,
        remote_module: demo_core::NODE.into(),
        watch: 0,
        expect_blocks: 0,
        probe: false,
        whoami: false,
        caller_gate: false,
        expect_denied: false,
        await_restart: 0,
    };
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut value = || it.next().unwrap_or_else(|| fail("args", format!("{flag} needs a value")));
        match flag.as_str() {
            "--runtime" => args.runtime = value().into(),
            "--modules" => args.modules = value().into(),
            "--data" => args.data = value().into(),
            "--tmp" => args.tmp = value().into(),
            "--name" => args.name = value(),
            "--hello" => args.hello = true,
            "--link-local" => args.link_local = Some(value().into()),
            "--invite" => args.invite = Some(value().into()),
            "--pair" => {
                let target = value();
                let (host, port) = target.rsplit_once(':').unwrap_or_else(|| fail("args", "--pair HOST:PORT".into()));
                args.pair = Some((host.into(), port.parse().unwrap_or_else(|_| fail("args", "bad port".into()))));
            }
            "--remote-module" => args.remote_module = value(),
            "--watch" => args.watch = value().parse().unwrap_or(0),
            "--expect-blocks" => args.expect_blocks = value().parse().unwrap_or(0),
            "--probe" => args.probe = true,
            "--whoami" => args.whoami = true,
            "--caller-gate" => args.caller_gate = true,
            "--expect-denied" => args.expect_denied = true,
            "--await-restart" => args.await_restart = value().parse().unwrap_or(0),
            other => fail("args", format!("unknown flag {other}")),
        }
    }
    args
}

fn main() {
    let args = parse_args();
    let blocks = Arc::new(AtomicU64::new(0));
    let fired = Arc::new(AtomicU64::new(0));
    let errors = Arc::new(AtomicU64::new(0));
    let (b, f, e) = (blocks.clone(), fired.clone(), errors.clone());
    let sink: demo_core::EventSink = Arc::new(move |event| match event {
        DemoEvent::NewBlock { count, .. } => b.store(count, Ordering::Relaxed),
        DemoEvent::HelloFired { count, .. } => f.store(count as u64, Ordering::Relaxed),
        DemoEvent::ImportState { name, state, reason } => {
            if state == "error" {
                e.fetch_add(1, Ordering::Relaxed);
            }
            emit("import_state", json!({"name": name, "state": state, "reason": reason}))
        }
        DemoEvent::RuntimeExited(reason) => emit("runtime_exited", json!(reason)),
        _ => {}
    });
    let paths = Paths::desktop(&args.runtime, &args.modules, &args.data, &args.tmp);
    let started = Instant::now();
    let demo = Demo::start("core_demo", &args.name, &paths, sink).unwrap_or_else(|e| fail("start", e));
    emit("start", json!({ "ms": started.elapsed().as_millis() as u64 }));

    if args.hello {
        demo.load_hello().unwrap_or_else(|e| fail("load_hello", e));
        let pong = demo.ping().unwrap_or_else(|e| fail("ping", e));
        let count = demo.fire("tag-1").unwrap_or_else(|e| fail("fire", e));
        let deadline = Instant::now() + Duration::from_secs(5);
        while fired.load(Ordering::Relaxed) < 1 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        emit("hello", json!({ "ping": pong, "fired": count, "events": fired.load(Ordering::Relaxed) }));
        if fired.load(Ordering::Relaxed) < 1 {
            fail("hello", "the fired event did not arrive".into());
        }
    }

    let peer = if let Some(file) = &args.link_local {
        Some(demo.link_local_daemon(file).unwrap_or_else(|e| fail("link_local", e)))
    } else if let Some(file) = &args.invite {
        let text = std::fs::read_to_string(file).unwrap_or_else(|e| fail("invite", e.to_string()));
        Some(demo.redeem_invite(text.lines().next().unwrap_or_default()).unwrap_or_else(|e| fail("invite", e)))
    } else if let Some((host, port)) = &args.pair {
        let pending = demo.pair_with(host, *port).unwrap_or_else(|e| fail("pair", e));
        emit("pair", json!({ "id": pending.id, "code": pending.code, "peer": pending.peer_display_id }));
        Some(demo.confirm_pairing(&pending, Duration::from_secs(120)).unwrap_or_else(|e| fail("pair", e)))
    } else {
        None
    };

    let Some(peer) = peer else { return };
    emit("paired", json!({ "peer": peer }));
    let exports = demo.peer_exports(&peer).unwrap_or_else(|e| fail("peer_exports", e));
    emit("peer_exports", exports);
    demo.import_node(&peer, &args.remote_module).unwrap_or_else(|e| fail("import", e));

    if args.expect_denied {
        // The peer's policy does not name its module for this runtime: the
        // import never becomes usable, and every call is refused.
        std::thread::sleep(Duration::from_secs(3));
        let reply = demo.core().call(demo_core::NODE, "get_chain_id", json!([]), Duration::from_secs(20));
        let state = demo.import_state(demo_core::NODE).unwrap_or_default();
        emit("denied", json!({ "refused": reply.is_err(), "reply": format!("{reply:?}"), "import": format!("{state:?}") }));
        if reply.is_ok() {
            fail("denied", "the call went through".into());
        }
        demo.stop();
        return;
    }

    demo.wait_import_ready(demo_core::NODE, Duration::from_secs(60)).unwrap_or_else(|e| fail("import", e));
    emit("import_ready", json!(demo_core::NODE));

    if args.caller_gate {
        // The import admits only the callers it names, checked when a caller's
        // session to the peer opens: the app's first call is refused while
        // bc_probe reads the node.
        demo.import_node_for(&peer, &args.remote_module, &["bc_probe"]).unwrap_or_else(|e| fail("callers", e));
        let shell = demo.core().call(demo_core::NODE, "get_chain_id", json!([]), Duration::from_secs(20));
        let probe = demo.probe().unwrap_or_else(|e| fail("callers", e));
        emit("callers", json!({ "shell_refused": shell.is_err(), "probe_ok": probe.get("ok") }));
        if shell.is_ok() || probe.get("ok").and_then(Value::as_bool) != Some(true) {
            fail("callers", format!("shell {shell:?}, probe {probe}"));
        }
        demo.import_node(&peer, &args.remote_module).unwrap_or_else(|e| fail("callers", e));
    }

    if args.whoami {
        let who = demo.core().call(demo_core::NODE, "whoami", json!([]), Duration::from_secs(20))
            .unwrap_or_else(|e| fail("whoami", e.to_string()));
        emit("whoami", who);
    }

    let snapshot = demo.read_node(None);
    emit("node", json!({
        "chain_id": snapshot.chain_id,
        "height": snapshot.chain.as_ref().map(|c| c.height),
        "mode": snapshot.chain.as_ref().map(|c| c.mode.clone()),
        "peers": snapshot.network.map(|n| n.peers),
        "lag": snapshot.lag(),
        "error": snapshot.error,
    }));
    if snapshot.chain.is_none() {
        fail("node", snapshot.error.unwrap_or_else(|| "no chain info".into()));
    }

    if args.probe {
        let probe = demo.probe().unwrap_or_else(|e| fail("probe", e));
        emit("probe", probe.clone());
        if probe.get("ok").and_then(Value::as_bool) != Some(true) {
            fail("probe", probe.to_string());
        }
    }

    if args.watch > 0 {
        demo.start_node_watch(Duration::from_secs(2)).unwrap_or_else(|e| fail("watch", e));
        let deadline = Instant::now() + Duration::from_secs(args.watch);
        while Instant::now() < deadline && (args.expect_blocks == 0 || blocks.load(Ordering::Relaxed) < args.expect_blocks) {
            std::thread::sleep(Duration::from_millis(100));
        }
        let seen = blocks.load(Ordering::Relaxed);
        emit("blocks", json!({ "new_block_events": seen }));
        if seen < args.expect_blocks {
            fail("blocks", format!("{seen} newBlock events, wanted {}", args.expect_blocks));
        }
    }
    if args.await_restart > 0 {
        // Something else restarts the peer: the import goes to error and back,
        // and its events resume.
        demo.start_node_watch(Duration::from_secs(2)).unwrap_or_else(|e| fail("restart", e));
        emit("awaiting_restart", Value::Null);
        let deadline = Instant::now() + Duration::from_secs(args.await_restart);
        while errors.load(Ordering::Relaxed) == 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(100));
        }
        if errors.load(Ordering::Relaxed) == 0 {
            fail("restart", "the import never reported the peer's loss".into());
        }
        demo.wait_import_ready(demo_core::NODE, deadline.saturating_duration_since(Instant::now()))
            .unwrap_or_else(|e| fail("restart", e));
        let before = blocks.load(Ordering::Relaxed);
        while blocks.load(Ordering::Relaxed) < before + 2 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(100));
        }
        let after = blocks.load(Ordering::Relaxed);
        emit("restart", json!({ "events_before": before, "events_after": after }));
        if after < before + 2 {
            fail("restart", "events did not resume".into());
        }
    }
    demo.stop();
    emit("stopped", Value::Null);
}
