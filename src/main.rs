//! space-game
//!
//! ```text
//! space_game                     host a game and play (listen server)
//! space_game --connect HOST[:PORT]   join a game
//! space_game --server            host without playing (headless)
//! space_game --port PORT         port to host on (default 5888)
//! ```

use std::net::{SocketAddr, ToSocketAddrs};
use core::time::Duration;

use bevy::app::ScheduleRunnerPlugin;
use bevy::prelude::*;
use space_game::client::ClientPlugin;
use space_game::net::{DEFAULT_PORT, NetMode, NetPlugin};

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
        app.add_plugins((
            MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_secs_f64(
                1.0 / 120.0,
            ))),
            bevy::log::LogPlugin::default(),
            bevy::state::app::StatesPlugin,
            TransformPlugin,
            AssetPlugin::default(),
            bevy::mesh::MeshPlugin,
            bevy::scene::ScenePlugin,
        ));
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
    app.add_plugins(NetPlugin { mode }).run();
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
