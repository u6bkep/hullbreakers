use avian3d::prelude::*;
use bevy::prelude::*;
use space_game::{client::ClientPlugin, sim::SimPlugin};

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins.set(WindowPlugin {
                primary_window: Some(Window {
                    title: "space-game".into(),
                    ..default()
                }),
                ..default()
            }),
            PhysicsPlugins::default().set(PhysicsInterpolationPlugin::interpolate_all()),
            SimPlugin::default(),
            ClientPlugin,
        ))
        .run();
}
