//! Where the player's bag is after they die: the game drops their inventory near a spot of
//! its own choosing, kept by `TheForest.Player.Actions.PlayerRetrieveDroppedInventoryAction`.

use game_maps_protocol::Vec3;
use unity_runtime::{Class, Field, Runtime, Unity, Vector3, WeakRef};

use crate::provider::to_proto;

pub struct DroppedBag {
    class: Class,
    is_dropped: Field,
    position: Field,
    /// The player's action object, once found. A weak reference: the game can drop the
    /// object at any time, and a raw pointer kept across frames would then point at
    /// whatever took its place.
    action: Option<WeakRef>,
    /// Searching every object takes long enough to feel: once per world, and after a death.
    searched: bool,
}

// Only used on the main thread.
unsafe impl Send for DroppedBag {}

impl DroppedBag {
    pub fn resolve(rt: &Runtime) -> Option<Self> {
        let class = rt.class("TheForest.Player.Actions", "PlayerRetrieveDroppedInventoryAction").ok()?;
        Some(Self {
            class,
            is_dropped: rt.field(class, "_isInventoryDropped").ok()?,
            position: rt.field(class, "_droppedInventoryPosition").ok()?,
            action: None,
            searched: false,
        })
    }

    pub fn forget(&mut self) {
        self.action = None;
        self.searched = false;
    }

    /// Where the bag is, while it's on the ground. `search_again`: the object wasn't there
    /// before, but may be now.
    ///
    /// # Safety
    /// Must run on the game's main thread, while in a world.
    pub unsafe fn read(&mut self, rt: &Runtime, unity: &Unity, search_again: bool) -> Option<Vec3> {
        let mut action = self.action.as_ref().map_or(std::ptr::null_mut(), |a| a.get());
        if !unity.is_alive(action) {
            // One that was there and went is looked for at once: the game may have made
            // another. One that was never found waits to be asked.
            let was_there = self.action.take().is_some();
            if self.searched && !search_again && !was_there {
                return None;
            }
            self.searched = true;
            action = unity.find_objects(rt, self.class, true).ok()?.first().copied()?;
            self.action = rt.weak_ref(action);
        }
        self.is_dropped.get::<bool>(action).then(|| to_proto(self.position.get::<Vector3>(action)))
    }
}
