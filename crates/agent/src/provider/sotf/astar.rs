//! Walking routes from the game's own pathfinding: Sons of the Forest uses the A*
//! Pathfinding Project (recast graphs, including the caves) instead of Unity's NavMesh.
//!
//! `ABPath.Construct(start, end, null)` → `AstarPath.StartPath(path, false)` →
//! `AstarPath.BlockUntilCalculated(path)`, then the corners in `path.vectorPath`.
//! Blocking runs the search right away on the main thread: about 4 ms on average, 15 ms for
//! the longest routes. The app asks at most every 1.5 s, and only after the player or the
//! target has moved (or every 15 s).

use std::ffi::c_void;

use unity_runtime::{read_value_array, Field, ManagedObject, Method, Runtime, Vector3};

/// `Pathfinding.PathCompleteState`.
const STATE_COMPLETE: i32 = 2;
const STATE_PARTIAL: i32 = 3;
/// Upper bound on route corners read back.
const MAX_CORNERS: usize = 4096;

pub struct AStar {
    construct: Method,
    start_path: Method,
    start_path_args: usize,
    block_until_calculated: Method,
    complete_state: Method,
    vector_path: Field,
    list_items: Option<(Field, Field)>,
}

// Only method and field handles (metadata), no objects.
unsafe impl Send for AStar {}

pub struct Route {
    pub corners: Vec<Vector3>,
    pub complete: bool,
}

impl AStar {
    /// Resolves the A* API by name; the error names the first missing piece.
    pub fn resolve(rt: &Runtime) -> Result<Self, String> {
        let class = |ns: &str, name: &str| rt.class(ns, name).map_err(|e| e.to_string());
        let ab = class("Pathfinding", "ABPath")?;
        let path = class("Pathfinding", "Path")?;
        let astar = class("", "AstarPath").or_else(|_| class("Pathfinding", "AstarPath"))?;
        let method = |c, name: &str, args: i32| rt.method(c, name, args).map_err(|e| e.to_string());
        // `StartPath(path, pushToFront)`, or with `assumeInPlayMode` in newer versions.
        let (start_path, start_path_args) = match method(astar, "StartPath", 2) {
            Ok(m) => (m, 2),
            Err(_) => (method(astar, "StartPath", 3)?, 3),
        };
        Ok(Self {
            construct: method(ab, "Construct", 3)?,
            start_path,
            start_path_args,
            block_until_calculated: method(astar, "BlockUntilCalculated", 1)?,
            complete_state: method(path, "get_CompleteState", 0)?,
            vector_path: rt.field(path, "vectorPath").map_err(|e| e.to_string())?,
            list_items: None,
        })
    }

    /// A route from `from` to `to`, computed now.
    ///
    /// # Safety
    /// Main thread only.
    pub unsafe fn find(&mut self, rt: &Runtime, from: Vector3, to: Vector3) -> Result<Route, String> {
        let (mut a, mut b) = (from, to);
        let mut args = [&mut a as *mut Vector3 as *mut c_void, &mut b as *mut Vector3 as *mut c_void, std::ptr::null_mut()];
        let path = rt.invoke(&self.construct, std::ptr::null_mut(), &mut args).map_err(|e| e.to_string())?;
        if path.is_null() {
            return Err("could not create a path".into());
        }
        // At the front of the queue: this blocks the game's main thread until it's done.
        let (mut front, mut in_play_mode) = (true, false);
        let mut args = [path as *mut c_void, &mut front as *mut bool as *mut c_void, &mut in_play_mode as *mut bool as *mut c_void];
        rt.invoke(&self.start_path, std::ptr::null_mut(), &mut args[..self.start_path_args]).map_err(|e| e.to_string())?;
        let mut args = [path as *mut c_void];
        rt.invoke(&self.block_until_calculated, std::ptr::null_mut(), &mut args).map_err(|e| e.to_string())?;

        let state: i32 = rt.unbox(rt.invoke(&self.complete_state, path, &mut []).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        if state != STATE_COMPLETE && state != STATE_PARTIAL {
            return Err(format!("no path (state {state})"));
        }
        let list: *mut ManagedObject = self.vector_path.get(path);
        if list.is_null() {
            return Err("path has no corners".into());
        }
        let (items, size) = match self.list_items {
            Some(f) => f,
            None => {
                // `List<Vector3>` stores its values inline in `_items`.
                let class = rt.class_of(list).ok_or("path corners have no class")?;
                let f = (rt.field(class, "_items").map_err(|e| e.to_string())?, rt.field(class, "_size").map_err(|e| e.to_string())?);
                *self.list_items.insert(f)
            }
        };
        let count = (size.get::<i32>(list).max(0) as usize).min(MAX_CORNERS);
        let corners = read_value_array::<Vector3>(items.get(list), count);
        Ok(Route { corners, complete: state == STATE_COMPLETE })
    }
}
