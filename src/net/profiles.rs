//! Profiles over the network (see [`crate::sim::progression`]).
//!
//! A client sends its profile once it has a player, and then whatever its
//! armory and workbench ask, on a reliable channel ([`ProfileWire`]). The
//! authority queues them for the simulation ([`ProfileOrders`]), by the
//! player of the link they came on. A host's own player's skip the network.
//! Profiles come back by replication (the [`Profile`] component); each
//! machine saves its own player's.

use bevy::prelude::*;
use lightyear::prelude::*;

use super::LocalPlayer;
use crate::client::save::ProfileStore;
use crate::sim::player::Player;
use crate::sim::progression::{Profile, ProfileFile, ProfileOp, ProfileOrders, ProfileWire};

/// The channel profiles travel on: reliable and in order (an upload before
/// what's asked of it).
pub struct ProfileChannel;

pub fn register(app: &mut App) {
    app.add_channel::<ProfileChannel>(ChannelSettings {
        mode: ChannelMode::OrderedReliable(ReliableSettings::default()),
        ..default()
    })
    .add_direction(NetworkDirection::ClientToServer);
    app.register_message::<ProfileWire>()
        .add_direction(NetworkDirection::ClientToServer);
    app.component::<Profile>().replicate();
}

/// What this machine's armory and workbench ask of its profile, to go to
/// the authority.
#[derive(Resource, Default)]
pub struct ProfileRequests(pub Vec<ProfileOp>);

/// Seconds between saves while things change (a gun fired changes what's
/// carried); a mission's end saves at once.
const SAVE_EVERY: f32 = 3.0;

/// A playing machine: its profile and requests go to the authority, by the
/// network or (a host) straight to its own simulation.
pub fn send_profile(
    authority: bool,
) -> impl FnMut(
    Option<ResMut<ProfileStore>>,
    Option<ResMut<ProfileRequests>>,
    Option<ResMut<ProfileOrders>>,
    Query<Entity, With<LocalPlayer>>,
    Query<&mut MessageSender<ProfileWire>, (With<Client>, With<Connected>)>,
) {
    move |store, requests, orders, local, mut senders| {
        let Ok(player) = local.single() else { return };
        let mut outgoing = Vec::new();
        if let Some(mut store) = store
            && !store.uploaded
        {
            store.uploaded = true;
            outgoing.push(ProfileWire::Upload(store.file.clone()));
        }
        if let Some(mut requests) = requests {
            outgoing.extend(requests.0.drain(..).map(ProfileWire::Op));
        }
        if outgoing.is_empty() {
            return;
        }
        if authority {
            if let Some(mut orders) = orders {
                orders.0.extend(outgoing.into_iter().map(|w| (player, w)));
            }
        } else if let Ok(mut sender) = senders.single_mut() {
            for wire in outgoing {
                sender.send::<ProfileChannel>(wire);
            }
        }
    }
}

/// Authority: what clients sent about their profiles, queued for their
/// players.
pub fn receive_profiles(
    mut links: Query<(Entity, &mut MessageReceiver<ProfileWire>)>,
    players: Query<(Entity, &ControlledBy), With<Player>>,
    mut orders: ResMut<ProfileOrders>,
) {
    for (link, mut receiver) in &mut links {
        let player = players.iter().find(|(_, c)| c.owner == link).map(|(e, _)| e);
        for wire in receiver.receive() {
            match player {
                Some(player) => orders.0.push((player, wire)),
                None => warn!("a profile message from {link:?}, which has no player"),
            }
        }
    }
}

/// A playing machine: keeps `profile.ron` as the authority has it (with
/// what this player carries), every few seconds while it changes, at once
/// when a mission ends, and on quitting.
pub fn save_profile(
    time: Res<Time>,
    store: Option<ResMut<ProfileStore>>,
    local: Query<(&Player, &Profile), With<LocalPlayer>>,
    mut exit: MessageReader<AppExit>,
) {
    let Some(mut store) = store else { return };
    store.since_save += time.delta_secs();
    let Ok((player, profile)) = local.single() else { return };
    // Not before what's carried has settled from the upload: the profile
    // and the player's own state may arrive a little apart.
    store.held_for += time.delta_secs();
    if store.held_for < 1.0 {
        return;
    }
    let file = ProfileFile::of(profile, &player.loadout.inventory);
    let quitting = exit.read().count() > 0;
    let mission_over = file.profile.missions != store.file.profile.missions;
    if quitting || mission_over || store.since_save >= SAVE_EVERY {
        store.save(file);
    }
}
