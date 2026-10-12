//! Profiles over the network (see [`crate::sim::progression`]).
//!
//! A client sends its profile once it has a player, and then whatever its
//! armory and workbench ask, on a reliable channel ([`ProfileWire`]). The
//! authority queues them for the simulation ([`ProfileOrders`]), by the
//! player of the link they came on. A host's own player's skip the network.
//! Profiles come back by replication (the [`Profile`] component); each
//! machine saves its own player's. A profile and its saved snapshot go
//! only to the player whose they are ([`ProfileOwner`]); what others need
//! of it is public: the player's name (their `Name`) and rank
//! ([`crate::sim::progression::Rank`]).

use bevy::prelude::*;
use bevy_replicon::prelude::{AppVisibilityExt, VisibilityFilter};
use lightyear::prelude::*;

use super::LocalPlayer;
use crate::client::save::ProfileStore;
use crate::sim::player::Player;
use crate::sim::progression::{Profile, ProfileOp, ProfileOrders, ProfileWire, SavedProfile};

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
    app.component::<SavedProfile>().replicate();
    app.component::<crate::sim::progression::Rank>().replicate();
}

/// Once every plugin is in: on a machine that sends replication (a server
/// or host), profiles go only to their owners. (A pure client has no
/// visibility to filter.)
pub fn finish(app: &mut App) {
    if app.world().contains_resource::<bevy_replicon::server::visibility::registry::FilterRegistry>() {
        app.add_visibility_filter::<ProfileOwner>();
    }
}

/// On a player: the link (client connection) their profile is replicated
/// to, and no other. A player without one (the host's own, a robot) sends
/// its profile to nobody.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[component(immutable)]
pub struct ProfileOwner(pub Entity);

impl VisibilityFilter for ProfileOwner {
    type ClientComponent = ProfileOwner;
    type Scope = (Profile, SavedProfile);

    fn is_visible(&self, client: Entity, _: Option<&ProfileOwner>) -> bool {
        client == self.0
    }
}

/// What this machine's armory and workbench ask of its profile, to go to
/// the authority.
#[derive(Resource, Default)]
pub struct ProfileRequests(pub Vec<ProfileOp>, pub Option<String>);

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
            outgoing.extend(requests.1.take().map(ProfileWire::Rename));
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

/// A playing machine: keeps `profile.ron` as the host's latest snapshot of
/// this player's (see [`SavedProfile`]: their profile and what they carry,
/// taken at one tick, as the host has it, never this machine's prediction).
pub fn save_profile(store: Option<ResMut<ProfileStore>>, local: Query<&SavedProfile, With<LocalPlayer>>) {
    let (Some(mut store), Ok(saved)) = (store, local.single()) else { return };
    if saved.file != store.file {
        store.save(saved.file.clone());
    }
}
