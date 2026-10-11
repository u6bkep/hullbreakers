//! space-game
//!
//! ```text
//! space_game                     host a game and play (listen server)
//! space_game --connect HOST[:PORT]   join a game
//! space_game --server            host without playing (headless)
//! space_game --port PORT         port to host on (default 5888)
//! ```
//!
//! `SPACE_GAME_SCENARIO=boarding` starts a host in the boarding action
//! rather than the asteroid field.

use std::net::{SocketAddr, ToSocketAddrs};
use core::time::Duration;

use bevy::prelude::*;
use space_game::client::ClientPlugin;
use space_game::sim::mission::{Director, ScenarioKind};
use space_game::net::{DEFAULT_PORT, NetMode, NetPlugin, SimulatedLag, add_headless_plugins};

fn main() {
    let mode = match parse_args(std::env::args().skip(1)) {
        Ok(mode) => mode,
        Err(err) => {
            eprintln!("space_game: {err}");
            std::process::exit(2);
        }
    };
    let mut app = App::new();
    if let NetMode::Server { .. } = mode {
        add_headless_plugins(&mut app, Duration::from_secs_f64(1.0 / 120.0));
        app.add_plugins(bevy::log::LogPlugin::default());
    } else {
        let title = match mode {
            NetMode::Client { server } => format!("space-game — {server}"),
            _ => "space-game".into(),
        };
        app.add_plugins((
            DefaultPlugins.set(WindowPlugin {
                primary_window: Some(Window { title, ..default() }),
                ..default()
            }),
            ClientPlugin,
        ));
    }
    app.add_plugins(NetPlugin {
        mode,
        lag: SimulatedLag::from_env(),
    });
    // `SPACE_GAME_SCENARIO=boarding`: start there instead of the field.
    if let Ok(name) = std::env::var("SPACE_GAME_SCENARIO") {
        let kind = match name.as_str() {
            "field" => ScenarioKind::Field,
            "boarding" => ScenarioKind::Boarding,
            other => {
                eprintln!("space_game: unknown scenario {other:?} (field, boarding)");
                std::process::exit(2);
            }
        };
        if let Some(mut director) = app.world_mut().get_resource_mut::<Director>() {
            director.start(kind);
        }
    }
    app.run();
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
