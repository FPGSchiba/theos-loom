//! The run's network (TU-03.5): the public web through the run's proxy, private ranges blocked,
//! every destination an `Egress` event, and no way out without the proxy. It needs no internet:
//! the "public" server sits on the egress network, whose subnet is `198.18.N.0/24` (a
//! benchmarking range, not a private one); the private server on a `10.N.0.0/24` network.
//! Linux only (D2): on other systems this file compiles to nothing.
#![cfg(target_os = "linux")]
// Test code throughout, helpers included, as in `support/mod.rs`.
#![cfg(test)]

#[path = "support/fake.rs"]
mod fake;
#[path = "support/podman.rs"]
mod podman;
mod support;
#[path = "support/world.rs"]
mod world;

use std::time::{SystemTime, UNIX_EPOCH};

use loom_core::run::{RunEvent, RunId, StopCause};
use loom_run::config::RunConfig;
use podman::{Leftovers, TEST_IMAGE, assert_nothing_left, launcher, podman};
use world::{World, ended, fake_line};

/// The port the test servers listen on.
const PORT: u16 = 8080;
/// How many subnets a network tries before the test gives up.
const SUBNET_TRIES: u32 = 64;

/// Two test servers (the fake in `serve` mode): one on a public-looking egress network, one on
/// a private network. Everything is labelled with an id of its own and removed on drop.
struct Servers {
    egress_network: String,
    public: String,
    private: String,
    _guard: Leftovers,
}

impl Servers {
    fn start() -> Self {
        let id = RunId::new();
        let guard = Leftovers(id);
        let label = format!("loom.run={id}");
        let egress_network = format!("loom-test-egress-{id}");
        let public = create_network(&egress_network, &label, |n| format!("198.18.{n}"));
        let private_network = format!("loom-test-private-{id}");
        let private = create_network(&private_network, &label, |n| format!("10.{n}.0"));
        serve(&egress_network, &public, &label);
        serve(&private_network, &private, &label);
        Self {
            egress_network,
            public,
            private,
            _guard: guard,
        }
    }

    /// A config of `world` whose egress network is the test's.
    fn config(&self, world: &World) -> RunConfig {
        let mut config = world.config();
        config.egress_network.clone_from(&self.egress_network);
        config
    }
}

/// Creates the network `name` on the first free `/24` of `prefix(n)` (tests run in parallel,
/// and other networks may hold a subnet); returns the server's address in it, `<prefix>.10`.
fn create_network(name: &str, label: &str, prefix: impl Fn(u32) -> String) -> String {
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("a clock after 1970")
        .subsec_nanos();
    let mut last = String::new();
    for attempt in 0..SUBNET_TRIES {
        let net = prefix(1 + (seed + attempt * 37) % 254);
        let subnet = format!("{net}.0/24");
        match podman(&[
            "network", "create", "--subnet", &subnet, "--label", label, name,
        ]) {
            Ok(_) => return format!("{net}.10"),
            Err(error) => last = error,
        }
    }
    panic!("no free subnet for {name}: {last}");
}

/// Starts the fake in `serve` mode at `ip` on `network`.
fn serve(network: &str, ip: &str, label: &str) {
    let fake = fake::fake_claude();
    let port = PORT.to_string();
    podman(&[
        "run",
        "-d",
        "--rm",
        "--label",
        label,
        "--network",
        network,
        "--ip",
        ip,
        "-v",
        &format!("{}:/usr/local/bin/claude:ro", fake.display()),
        "--env",
        "FAKE_MODE=serve",
        "--env",
        &format!("FAKE_PORT={port}"),
        TEST_IMAGE,
        "claude",
        "-p",
    ])
    .unwrap_or_else(|error| panic!("the test server at {ip} did not start: {error}"));
}

fn egress(events: &[RunEvent]) -> Vec<&RunEvent> {
    events
        .iter()
        .filter(|event| matches!(event, RunEvent::Egress { .. }))
        .collect()
}

// AC: TU-03.5
#[tokio::test(flavor = "multi_thread")]
async fn a_request_to_a_private_address_is_blocked_and_logged() {
    let servers = Servers::start();
    let world = World::new().await;
    let config = servers.config(&world);
    let url = format!("http://{}:{PORT}/", servers.private);
    let fake = launcher(&config, "request").with_env("FAKE_URL", &url);

    let (id, events, _guard) = world.run(config, fake).await;

    assert_eq!(ended(&events), &StopCause::Completed, "{events:#?}");
    assert_eq!(
        fake_line(&events, "fake_request")["status"],
        403,
        "{events:#?}"
    );
    let blocked = RunEvent::Egress {
        host: servers.private.clone(),
        port: Some(PORT),
        allowed: false,
    };
    assert_eq!(egress(&events), vec![&blocked], "{events:#?}");
    assert_nothing_left(id);
    world.end().await;
}

// AC: TU-03.5
#[tokio::test(flavor = "multi_thread")]
async fn a_request_to_a_public_address_goes_through_and_is_logged() {
    let servers = Servers::start();
    let world = World::new().await;
    let config = servers.config(&world);
    let url = format!("http://{}:{PORT}/", servers.public);
    let fake = launcher(&config, "request").with_env("FAKE_URL", &url);

    let (id, events, _guard) = world.run(config, fake).await;

    assert_eq!(ended(&events), &StopCause::Completed, "{events:#?}");
    assert_eq!(
        fake_line(&events, "fake_request")["status"],
        200,
        "{events:#?}"
    );
    let allowed = RunEvent::Egress {
        host: servers.public.clone(),
        port: Some(PORT),
        allowed: true,
    };
    assert_eq!(egress(&events), vec![&allowed], "{events:#?}");
    assert_nothing_left(id);
    world.end().await;
}

// AC: TU-03.5
#[tokio::test(flavor = "multi_thread")]
async fn a_direct_connection_without_the_proxy_fails() {
    let servers = Servers::start();
    let world = World::new().await;
    let config = servers.config(&world);
    let addr = format!("{}:{PORT}", servers.public);
    let fake = launcher(&config, "connect").with_env("FAKE_ADDR", &addr);

    let (id, events, _guard) = world.run(config, fake).await;

    assert_eq!(ended(&events), &StopCause::Completed, "{events:#?}");
    assert_eq!(
        fake_line(&events, "fake_connect")["ok"],
        false,
        "{events:#?}"
    );
    assert_eq!(egress(&events), Vec::<&RunEvent>::new(), "{events:#?}");
    assert_nothing_left(id);
    world.end().await;
}
