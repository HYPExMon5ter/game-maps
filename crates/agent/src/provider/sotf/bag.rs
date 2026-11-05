//! Where the player's bag is after they die: the game drops their inventory near a spot of
//! its own choosing, kept by `TheForest.Player.Actions.PlayerRetrieveDroppedInventoryAction`.

use game_maps_protocol::Vec3;
use unity_runtime::{Class, Field, ManagedObject, Runtime, Unity, Vector3};

use crate::provider::to_proto;

pub struct DroppedBag {
    class: Class,
    is_dropped: Field,
    position: Field,
    /// The player's action object, once found. Null when there's none yet; it doesn't
    /// outlive the world (see [`DroppedBag::forget`]).
    action: *mut ManagedObject,
    /// Searching every object takes long enough to feel: once per world, and after a death.
    searched: bool,
}

// Raw object pointers are only dereferenced on the main thread.
unsafe impl Send for DroppedBag {}

impl DroppedBag {
    pub fn resolve(rt: &Runtime) -> Option<Self> {
        let class = rt.class("TheForest.Player.Actions", "PlayerRetrieveDroppedInventoryAction").ok()?;
        Some(Self {
            class,
            is_dropped: rt.field(class, "_isInventoryDropped").ok()?,
            position: rt.field(class, "_droppedInventoryPosition").ok()?,
            action: std::ptr::null_mut(),
            searched: false,
        })
    }

    pub fn forget(&mut self) {
        self.action = std::ptr::null_mut();
        self.searched = false;
    }

    /// Where the bag is, while it's on the ground. `search_again`: the object wasn't there
    /// before, but may be now.
    ///
    /// # Safety
    /// Must run on the game's main thread, while in a world.
    pub unsafe fn read(&mut self, rt: &Runtime, unity: &Unity, search_again: bool) -> Option<Vec3> {
        if self.action.is_null() || !unity.is_alive(self.action) {
            self.action = std::ptr::null_mut();
            if self.searched && !search_again {
                return None;
            }
            self.searched = true;
            self.action = unity.find_objects(rt, self.class, true).ok()?.first().copied()?;
        }
        self.is_dropped.get::<bool>(self.action).then(|| to_proto(self.position.get::<Vector3>(self.action)))
    }
}
