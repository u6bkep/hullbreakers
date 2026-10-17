//! Hullbreakers
//!
//! ```text
//! hullbreakers                     host a game and play (listen server)
//! hullbreakers --connect HOST[:PORT]   join a game
//! hullbreakers --server            host without playing (headless)
//! hullbreakers --port PORT         port to host on (default 5888)
//! ```
//!
//! `HULLBREAKERS_SCENARIO=boarding` starts a host in the boarding action
//! rather than the asteroid field. `HULLBREAKERS_PRESENT=novsync` (or
//! `vsync`, `immediate`, `mailbox`, `fifo`) overrides the window's present
//! mode, for measuring (see `tools/stress`).

use std::net::{SocketAddr, ToSocketAddrs};
use core::time::Duration;

use bevy::prelude::*;
use hullbreakers::client::ClientPlugin;
use hullbreakers::sim::mission::{Director, ScenarioKind};
use hullbreakers::net::{DEFAULT_PORT, NetMode, NetPlugin, SimulatedLag, add_headless_plugins};

fn main() {
    let mode = match parse_args(std::env::args().skip(1)) {
        Ok(mode) => mode,
        Err(err) => {
            eprintln!("hullbreakers: {err}");
            std::process::exit(2);
        }
    };
    let mut app = App::new();
    if let NetMode::Server { .. } = mode {
        add_headless_plugins(&mut app, Duration::from_secs_f64(1.0 / 120.0));
        app.add_plugins(bevy::log::LogPlugin::default());
    } else {
        let title = match mode {
            NetMode::Client { server } => format!("Hullbreakers — {server}"),
            _ => "Hullbreakers".into(),
        };
        let present_mode = present_mode_from_env();
        app.add_plugins((
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title,
                        present_mode,
                        ..default()
                    }),
                    ..default()
                })
                .set(AssetPlugin {
                    file_path: hullbreakers::client::gallery::asset_root().to_string_lossy().into(),
                    ..default()
                }),
            ClientPlugin,
        ));
    }
    app.add_plugins(NetPlugin {
        mode,
        lag: SimulatedLag::from_env(),
    });
    // `HULLBREAKERS_SCENARIO=boarding`: start there instead of the field.
    if let Ok(name) = std::env::var("HULLBREAKERS_SCENARIO") {
        let Some(kind) = ScenarioKind::from_id(&name) else {
            let known: Vec<&str> = ScenarioKind::ALL.iter().map(|k| k.id()).collect();
            eprintln!("hullbreakers: unknown scenario {name:?} ({})", known.join(", "));
            std::process::exit(2);
        };
        if let Some(mut director) = app.world_mut().get_resource_mut::<Director>() {
            director.start(kind);
        }
    }
    // `HULLBREAKERS_SCENE=<seed>[:<size>]`: start in that scene, and keep
    // to it (the wall's button rebuilds the same one).
    if let Ok(text) = std::env::var("HULLBREAKERS_SCENE") {
        let seed = hullbreakers::sim::scene::SceneSeed::parse(&text).unwrap_or_else(|e| {
            eprintln!("hullbreakers: HULLBREAKERS_SCENE: {e}");
            std::process::exit(2);
        });
        if let Some(mut director) = app.world_mut().get_resource_mut::<Director>() {
            director.start_scene(seed);
        }
    }
    app.run();
}

/// `HULLBREAKERS_PRESENT`: the window's present mode (Bevy's default,
/// `AutoVsync`, when unset).
fn present_mode_from_env() -> bevy::window::PresentMode {
    use bevy::window::PresentMode;
    match std::env::var("HULLBREAKERS_PRESENT").as_deref() {
        Err(_) | Ok("") | Ok("vsync") => PresentMode::AutoVsync,
        Ok("novsync") => PresentMode::AutoNoVsync,
        Ok("immediate") => PresentMode::Immediate,
        Ok("mailbox") => PresentMode::Mailbox,
        Ok("fifo") => PresentMode::Fifo,
        Ok(other) => {
            eprintln!("hullbreakers: unknown HULLBREAKERS_PRESENT {other:?} (vsync, novsync, immediate, mailbox, fifo)");
            PresentMode::AutoVsync
        }
    }
}

fn parse_args(mut args: impl Iterator<Item = String>) -> Result<NetMode, String> {
    let mut port = DEFAULT_PORT;
    let mut connect = None;
    let mut server = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--connect" => connect = Some(args.next().ok_or("--connect needs HOST[:PORT]")?),
            "--server" => server = true,
            "--port" => {
                port = args
                    .next()
                    .and_then(|p| p.parse().ok())
                    .ok_or("--port needs a number")?
            }
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    Ok(match (connect, server) {
        (Some(_), true) => return Err("--connect and --server don't mix".into()),
        (Some(target), false) => NetMode::Client {
            server: resolve(&target)?,
        },
        (None, true) => NetMode::Server { port },
        (None, false) => NetMode::Host { port },
    })
}

fn resolve(target: &str) -> Result<SocketAddr, String> {
    let with_port = if target.contains(':') {
        target.to_owned()
    } else {
        format!("{target}:{DEFAULT_PORT}")
    };
    with_port
        .to_socket_addrs()
        .map_err(|e| format!("can't resolve {target}: {e}"))?
        .next()
        .ok_or_else(|| format!("{target} has no address"))
}
