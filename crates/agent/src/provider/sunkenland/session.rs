//! The Photon Fusion session: `Fusion.NetworkRunner.Instances`, the running runner's
//! `SessionInfo.Name` (the same for everyone in the game), and whether this player hosts
//! or joined. Single player runs a runner too, in single player mode.

use game_maps_protocol::MultiplayerSession;
use unity_runtime::{read_string, ListLayout, ManagedObject, Method, Runtime};

const MAX_RUNNERS: usize = 8;

pub struct Session {
    instances: Method,
    is_running: Method,
    is_client: Method,
    is_single_player: Method,
    session_info: Method,
    session_name: Method,
    persona_name: Option<Method>,
    layout: Option<ListLayout>,
}

/// What the running session says about this game.
pub struct SessionState {
    /// `None` in single player.
    pub session: Option<MultiplayerSession>,
    /// In someone else's world.
    pub joined: bool,
}

impl Session {
    pub fn resolve(rt: &Runtime) -> Option<Self> {
        let runner = rt.class("Fusion", "NetworkRunner").ok()?;
        let info = rt.class("Fusion", "SessionInfo").ok()?;
        Some(Self {
            instances: rt.method(runner, "get_Instances", 0).ok().filter(|m| m.is_static)?,
            is_running: rt.method(runner, "get_IsRunning", 0).ok()?,
            is_client: rt.method(runner, "get_IsClient", 0).ok()?,
            is_single_player: rt.method(runner, "get_IsSinglePlayer", 0).ok()?,
            session_info: rt.method(runner, "get_SessionInfo", 0).ok()?,
            session_name: rt.method(info, "get_Name", 0).ok()?,
            persona_name: rt.class("Steamworks", "SteamFriends").ok().and_then(|c| rt.method(c, "GetPersonaName", 0).ok()).filter(|m| m.is_static),
            layout: None,
        })
    }

    unsafe fn flag(rt: &Runtime, m: &Method, runner: *mut ManagedObject) -> bool {
        rt.invoke(m, runner, &mut []).and_then(|b| rt.unbox::<bool>(b)).unwrap_or(false)
    }

    /// The running session, if there is one.
    ///
    /// # Safety
    /// Must run on the game's main thread.
    pub unsafe fn read(&mut self, rt: &Runtime) -> Option<SessionState> {
        let list = rt.invoke(&self.instances, std::ptr::null_mut(), &mut []).ok().filter(|l| !l.is_null())?;
        let layout = match self.layout {
            Some(l) => l,
            None => *self.layout.insert(ListLayout::of(rt, list).ok()?),
        };
        let runner = layout.elements(list, MAX_RUNNERS).into_iter().find(|&r| !r.is_null() && Self::flag(rt, &self.is_running, r))?;
        if Self::flag(rt, &self.is_single_player, runner) {
            return Some(SessionState { session: None, joined: false });
        }
        let info = rt.invoke(&self.session_info, runner, &mut []).ok().filter(|i| !i.is_null())?;
        let name = read_string(rt.invoke(&self.session_name, info, &mut []).ok()?).filter(|n| !n.is_empty())?;
        let player_name = self.persona_name.as_ref().and_then(|m| read_string(rt.invoke(m, std::ptr::null_mut(), &mut []).ok()?)).filter(|n| !n.is_empty());
        Some(SessionState { session: Some(MultiplayerSession { id: format!("session:{name}"), player_name }), joined: Self::flag(rt, &self.is_client, runner) })
    }
}
