//! Which multiplayer session the player is in, so copies of the app in the same game can
//! find each other: the Steam lobby everyone joined (`Sons.Multiplayer.CoopLobbyManager`
//! active lobby), or the dedicated server's Steam id. Also the player's own Steam name.

use unity_runtime::{read_string, Field, ManagedObject, Method, Runtime};

pub struct Session {
    active_lobby: Method,
    lobby_id: Method,
    lobby_destroyed: Option<Field>,
    /// `GameServerManager.ServerId` (a `CSteamID`), set when connected to a dedicated server.
    server_id: Option<Field>,
    persona_name: Option<Method>,
}

impl Session {
    pub fn resolve(rt: &Runtime) -> Option<Self> {
        let manager = rt.class("Sons.Multiplayer", "CoopLobbyManager").ok()?;
        let lobby = rt.class("Sons.Multiplayer", "CoopLobby").ok()?;
        let server = rt.class("Sons.Multiplayer", "GameServerManager").ok();
        Some(Self {
            active_lobby: rt.method(manager, "GetActiveInstance", 0).ok().filter(|m| m.is_static)?,
            lobby_id: rt.method(lobby, "get_LobbyId", 0).ok()?,
            lobby_destroyed: rt.field(lobby, "_destroyed").ok(),
            server_id: server.and_then(|s| rt.field(s, "ServerId").ok()).filter(|f| f.is_static),
            persona_name: rt.class("Steamworks", "SteamFriends").ok().and_then(|c| rt.method(c, "GetPersonaName", 0).ok()).filter(|m| m.is_static),
        })
    }

    /// The session everyone in this game shares, or `None` in single player.
    ///
    /// # Safety
    /// Must run on the game's main thread.
    pub unsafe fn id(&self, rt: &Runtime) -> Option<String> {
        let lobby = rt.invoke(&self.active_lobby, std::ptr::null_mut(), &mut []).ok().filter(|l| !l.is_null());
        if let Some(lobby) = lobby.filter(|&l| !self.lobby_destroyed.is_some_and(|f| f.get::<bool>(l))) {
            if let Some(id) = steam_id(rt, &self.lobby_id, lobby) {
                return Some(format!("lobby:{id}"));
            }
        }
        let server: u64 = self.server_id.map(|f| rt.static_value(f)).unwrap_or(0);
        (server != 0).then(|| format!("server:{server}"))
    }

    /// The player's Steam name.
    ///
    /// # Safety
    /// Must run on the game's main thread.
    pub unsafe fn player_name(&self, rt: &Runtime) -> Option<String> {
        let m = self.persona_name.as_ref()?;
        read_string(rt.invoke(m, std::ptr::null_mut(), &mut []).ok()?).filter(|n| !n.is_empty())
    }
}

/// A `CSteamID` (one `u64`) returned by `getter`, if set.
unsafe fn steam_id(rt: &Runtime, getter: &Method, instance: *mut ManagedObject) -> Option<u64> {
    let id: u64 = rt.unbox(rt.invoke(getter, instance, &mut []).ok()?).ok()?;
    (id != 0).then_some(id)
}
