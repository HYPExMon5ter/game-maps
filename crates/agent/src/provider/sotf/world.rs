//! Which save is loaded: `GameSetupManager.GetSelectedSaveId()` (the save's folder id)
//! and `GetSaveGameType()` (`SinglePlayer`, `Multiplayer` when hosting, or
//! `MultiplayerClient` when joining; clients keep a save of their own per host game).

use game_maps_protocol::{GameWorld, WorldKind};
use unity_runtime::{Class, Field, ManagedObject, Method, Runtime};

/// Namespaces `GameSetupManager` has lived in.
const NAMESPACES: &[&str] = &["Sons.Gameplay.GameSetup", "Sons.Save", "TheForest.Utils", "Sons.Gameplay", ""];

pub struct World {
    save_id: Method,
    save_type: Option<Method>,
    /// Static instance field, when the getters aren't static.
    instance: Option<Field>,
}

impl World {
    /// Resolves the members; the error says what's missing.
    pub fn resolve(rt: &Runtime) -> Result<(Self, &'static str), String> {
        let (class, namespace): (Class, &str) = NAMESPACES
            .iter()
            .find_map(|ns| rt.class(ns, "GameSetupManager").ok().map(|c| (c, *ns)))
            .ok_or("GameSetupManager not found")?;
        let save_id = rt.method(class, "GetSelectedSaveId", 0).map_err(|e| e.to_string())?;
        let save_type = rt.method(class, "GetSaveGameType", 0).ok();
        let instance = if save_id.is_static {
            None
        } else {
            Some(["_instance", "Instance", "instance"].iter().find_map(|n| rt.field(class, n).ok().filter(|f| f.is_static)).ok_or("GameSetupManager instance not found")?)
        };
        Ok((Self { save_id, save_type, instance }, namespace))
    }

    /// What the save id getter returns, for the log (`type: value`).
    ///
    /// # Safety
    /// Must run on the game's main thread.
    pub unsafe fn describe(&self, rt: &Runtime) -> String {
        let this = match self.instance {
            Some(f) => rt.static_value::<*mut ManagedObject>(f),
            None => std::ptr::null_mut(),
        };
        if self.instance.is_some() && this.is_null() {
            return "no GameSetupManager instance".into();
        }
        match rt.invoke(&self.save_id, this, &mut []) {
            Ok(boxed) => {
                let class = rt.class_of(boxed).map(|c| rt.class_name(c)).unwrap_or_else(|| "null".into());
                let kind = self.save_type.as_ref().and_then(|m| rt.enum_name(rt.invoke(m, this, &mut []).ok()?));
                format!("{class}: {:?}, type {kind:?}", rt.unbox_integer(boxed))
            }
            Err(e) => format!("error: {e}"),
        }
    }

    /// The loaded save, or `None` if there's no valid save id yet.
    ///
    /// # Safety
    /// Must run on the game's main thread, while in a world.
    pub unsafe fn read(&self, rt: &Runtime) -> Option<GameWorld> {
        let this: *mut ManagedObject = match self.instance {
            Some(f) => {
                let i: *mut ManagedObject = rt.static_value(f);
                if i.is_null() {
                    return None;
                }
                i
            }
            None => std::ptr::null_mut(),
        };
        let id = rt.unbox_integer(rt.invoke(&self.save_id, this, &mut []).ok()?)?;
        if id <= 0 {
            return None;
        }
        let kind_name = self.save_type.as_ref().and_then(|m| rt.enum_name(rt.invoke(m, this, &mut []).ok()?));
        let kind = match kind_name.as_deref() {
            Some("Multiplayer") => WorldKind::Hosted,
            Some("MultiplayerClient") => WorldKind::Joined,
            _ => WorldKind::SinglePlayer,
        };
        // Save ids are per save type (each has its own folder), so the type is part of
        // the identity.
        let prefix = match kind {
            WorldKind::SinglePlayer => "sp",
            WorldKind::Hosted => "mp",
            WorldKind::Joined => "client",
        };
        Some(GameWorld { id: format!("{prefix}:{id}"), name: None, kind })
    }
}
