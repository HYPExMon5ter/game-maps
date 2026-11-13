//! Grounded 2 (Unreal Engine 5).

mod creatures;
mod export;
mod markers;
mod world;

use std::time::{Duration, Instant};

use game_maps_protocol::{Capabilities, PathInfo, CompanionOrderOption, CompanionStatus, Creature, Environment, LocalPlayer, Marker, MarkerKind, RemotePlayer, TeleportTarget, Vec3, WorldExport, WorldLayer, WorldSnapshot};
use unreal_runtime::{Args, FRotator, Function, Obj, Property, TArray, Unreal};

use super::Provider;
use crate::engine::Engine;
use crate::unreal::{dir_to_proto, from_proto, to_proto, UeCx, UnrealEngine};
use creatures::{CreatureMembers, Creatures, BUGGY};
use markers::{MarkerMembers, Markers};
use world::WorldMembers;

/// Snapping to the ground looks down from this high (meters), to this far below.
const SNAP_TOP: f32 = 1000.0;
const SNAP_BOTTOM: f32 = -1000.0;
/// How long a teleport waits for the destination's ground to stream in.
const LANDING_WAIT: Duration = Duration::from_secs(10);
/// `EMovementMode`
const MOVE_WALKING: u8 = 1;
const MOVE_FLYING: u8 = 5;
/// Teleports land this far (meters) above the destination, so the player drops onto it.
const GROUND_CLEARANCE: f32 = 0.5;
/// How often what changes slowly is read again: creatures, markers, the clock.
const SLOW_REFRESH: Duration = Duration::from_millis(500);
/// How often the world's loot bags are looked through for the player's backpack.
const BACKPACK_REFRESH: Duration = Duration::from_secs(5);

/// Members the provider uses, resolved once.
struct Members {
    game_state: Property,
    player_array: Property,
    pawn_of_state: Property,
    player_name: Property,
    teleport: std::sync::Arc<Function>,
    character_movement: Property,
    set_movement_mode: std::sync::Arc<Function>,
    player_state: Property,
    /// `SurvivalPlayerCharacter.ServerRequestTeleport`: how a client asks the host.
    request_teleport: Option<std::sync::Arc<Function>>,
    is_dead: Option<std::sync::Arc<Function>>,
    world: Option<WorldMembers>,
    creatures: Option<CreatureMembers>,
    markers: Option<MarkerMembers>,
}

impl Members {
    fn resolve(ue: &Unreal) -> unreal_runtime::Result<Self> {
        let world = ue.find_struct("/Script/Engine.World")?;
        let state = ue.find_struct("/Script/Engine.PlayerState")?;
        Ok(Self {
            game_state: ue.property_of(world, "GameState", "Object<GameStateBase>")?,
            player_array: ue.property_of(ue.find_struct("/Script/Engine.GameStateBase")?, "PlayerArray", "Array<Object<PlayerState>>")?,
            pawn_of_state: ue.property_of(state, "PawnPrivate", "Object<Pawn>")?,
            player_name: ue.property_of(state, "PlayerNamePrivate", "Str")?,
            teleport: ue.function(ue.find_struct("/Script/Engine.Actor")?, "K2_TeleportTo")?,
            character_movement: ue.property_of(ue.find_struct("/Script/Engine.Character")?, "CharacterMovement", "Object<CharacterMovementComponent>")?,
            set_movement_mode: ue.function(ue.find_struct("/Script/Engine.CharacterMovementComponent")?, "SetMovementMode")?,
            player_state: ue.property_of(ue.find_struct("/Script/Engine.Controller")?, "PlayerState", "Object<PlayerState>")?,
            request_teleport: ue.find_struct("/Script/Maine.SurvivalPlayerCharacter").ok().and_then(|c| ue.function(c, "ServerRequestTeleport").ok()),
            is_dead: ue.find_struct("/Script/Maine.SurvivalCharacter").ok().and_then(|c| ue.function(c, "IsDead").ok()),
            world: None,
            creatures: None,
            markers: None,
        })
    }

    /// Resolves each feature's members on its own, so a game update costs only the
    /// features whose members moved.
    fn resolve_features(&mut self, ue: &Unreal, notes: &mut Vec<String>) {
        let mut feature = |name: &str, r: unreal_runtime::Result<()>| {
            if let Err(e) = r {
                notes.push(format!("Grounded 2 {name} unavailable: {e}"));
            }
        };
        feature("world detection", WorldMembers::resolve(ue).map(|w| self.world = Some(w)));
        feature("creatures", CreatureMembers::resolve(ue).map(|c| self.creatures = Some(c)));
        feature("markers", MarkerMembers::resolve(ue).map(|m| self.markers = Some(m)));
    }
}

/// What's read every `SLOW_REFRESH` rather than every tick.
#[derive(Default)]
struct Slow {
    at: Option<Instant>,
    creatures: Vec<Creature>,
    markers: Vec<Marker>,
    backpack: Option<Vec3>,
    /// The backpack as found among the world's loot bags, and when they were last looked through.
    bag: Option<Vec3>,
    bag_scan: Option<Instant>,
    buggy: Option<CompanionStatus>,
    environment: Option<Environment>,
}

pub struct Grounded2 {
    m: Members,
    /// Other players as last read, for teleporting to them.
    others: Vec<RemotePlayer>,
    /// Their characters.
    other_pawns: Vec<Obj>,
    /// The last snapshot with the player in it, for while they're dead.
    last: Option<WorldSnapshot>,
    notes: Vec<String>,
    version_logged: bool,
    /// A teleport waiting for its destination's ground to stream in.
    landing: Option<Landing>,
    /// A finished world export, handed over on the next poll.
    export: Option<Result<WorldExport, String>>,
    slow: Slow,
    creature_reader: Creatures,
    marker_reader: Markers,
}

/// The player held in the air over a teleport destination until the ground is there.
struct Landing {
    at: Vec3,
    /// Where the player was, to go back to if the ground never loads.
    from: Vec3,
    until: Instant,
}

impl Grounded2 {
    pub fn detect(engine: &UnrealEngine) -> Option<Self> {
        let exe = std::env::current_exe().ok()?;
        let name = exe.file_name()?.to_string_lossy().to_ascii_lowercase();
        if !name.starts_with("grounded2") {
            return None;
        }
        let mut notes: Vec<String> = std::mem::take(&mut *engine.notes.lock().unwrap());
        match Members::resolve(&engine.ue) {
            Ok(mut m) => {
                m.resolve_features(&engine.ue, &mut notes);
                Some(Self {
                    m,
                    others: Vec::new(),
                    other_pawns: Vec::new(),
                    last: None,
                    notes,
                    version_logged: false,
                    landing: None,
                    export: None,
                    slow: Slow::default(),
                    creature_reader: Creatures::default(),
                    marker_reader: Markers::default(),
                })
            }
            Err(e) => {
                notes.push(format!("Grounded 2: {e}"));
                crate::filelog::write(&format!("Grounded 2 provider unavailable: {e}"));
                None
            }
        }
    }

    /// The highest solid surface at `at`'s spot, if that part of the world is loaded.
    unsafe fn ground(cx: &UeCx, at: Vec3) -> Option<f32> {
        let top = Vec3::new(at.x, (SNAP_TOP + SNAP_BOTTOM) / 2.0, at.z);
        cx.ground_below(top, (SNAP_TOP - SNAP_BOTTOM) / 2.0)
    }

    unsafe fn set_movement_mode(&self, cx: &UeCx, pawn: Obj, mode: u8) -> Result<(), String> {
        let movement = cx.ue.get_obj(pawn, &self.m.character_movement).ok_or("the player has no movement component")?;
        let mut args = Args::new(&self.m.set_movement_mode);
        args.set("NewMovementMode", mode).map_err(|e| e.to_string())?;
        cx.ue.call(movement, &mut args).map_err(|e| e.to_string())
    }

    unsafe fn move_pawn(&self, cx: &UeCx, pawn: Obj, position: Vec3) -> Result<(), String> {
        if !(position.x.is_finite() && position.y.is_finite() && position.z.is_finite()) {
            return Err("invalid destination".into());
        }
        let rotation = cx.rotation(pawn).unwrap_or_default();
        // A client can't move itself: the host would put it back. It asks the host the
        // way the game's own teleporters do.
        if cx.is_server(pawn) == Some(false) {
            let f = self.m.request_teleport.as_ref().ok_or("teleporting in someone else's world isn't possible in this game version")?;
            let mut args = Args::new(f);
            args.set("DestLocation", from_proto(position)).map_err(|e| e.to_string())?;
            args.set("DestRotation", rotation).map_err(|e| e.to_string())?;
            return cx.ue.call(pawn, &mut args).map_err(|e| e.to_string());
        }
        let mut args = Args::new(&self.m.teleport);
        args.set("DestLocation", from_proto(position)).map_err(|e| e.to_string())?;
        args.set("DestRotation", rotation).map_err(|e| e.to_string())?;
        cx.ue.call(pawn, &mut args).map_err(|e| e.to_string())?;
        if args.get_bool("ReturnValue").unwrap_or(false) {
            Ok(())
        } else {
            Err("the game refused the teleport".into())
        }
    }

    /// Sets a held player down once the ground under them has loaded, or lets go.
    unsafe fn land(&mut self, cx: &UeCx, pawn: Obj) {
        let Some(landing) = &self.landing else { return };
        let (at, from) = (landing.at, landing.from);
        let ground = Self::ground(cx, at);
        if ground.is_none() && Instant::now() < landing.until {
            return;
        }
        self.landing = None;
        if let Some(y) = ground {
            if let Err(e) = self.move_pawn(cx, pawn, Vec3::new(at.x, y + GROUND_CLEARANCE, at.z)) {
                self.notes.push(format!("teleport landing failed: {e}"));
            }
        } else {
            self.notes.push("teleport: the destination never loaded; going back".into());
            if let Err(e) = self.move_pawn(cx, pawn, from) {
                self.notes.push(format!("teleport: going back failed: {e}"));
            }
        }
        if let Err(e) = self.set_movement_mode(cx, pawn, MOVE_WALKING) {
            self.notes.push(format!("teleport: could not restore walking: {e}"));
        }
    }


    /// Other players, with their characters.
    unsafe fn read_others(&self, cx: &UeCx, world: Obj, local_pawn: Option<Obj>) -> Vec<(RemotePlayer, Obj)> {
        let ue = &cx.ue;
        let Some(state) = ue.get_obj(world, &self.m.game_state) else { return Vec::new() };
        let states: TArray<Obj> = ue.get(state, &self.m.player_array);
        states
            .to_vec()
            .into_iter()
            .filter(|&s| s != 0 && ue.alive(s))
            .filter_map(|s| {
                let pawn = ue.get_obj(s, &self.m.pawn_of_state).filter(|&p| Some(p) != local_pawn)?;
                let position = cx.location(pawn)?;
                let forward = cx.rotation(pawn).map(|r| dir_to_proto(FRotator { pitch: 0.0, ..r }.forward()));
                let name = ue.get::<unreal_runtime::FString>(s, &self.m.player_name).to_string_lossy();
                Some((RemotePlayer { id: format!("{s:x}"), name, color: None, position: to_proto(position), forward }, pawn))
            })
            .collect()
    }
}

impl Provider<UnrealEngine> for Grounded2 {
    fn game_id(&self) -> &'static str {
        "grounded-2"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities { live_location: true, teleport: true, other_players: true, hotkeys: true, game_map: false, game_pings: self.m.markers.is_some() }
    }

    fn take_notes(&mut self) -> Vec<String> {
        std::mem::take(&mut self.notes)
    }

    unsafe fn snapshot(&mut self, cx: &UeCx, frame: u64) -> WorldSnapshot {
        if !self.version_logged {
            self.version_logged = true;
            self.notes.push(format!("Unreal Engine {}", cx.version().unwrap_or_default()));
        }
        let Some(local) = cx.local() else {
            self.last = None;
            return WorldSnapshot { frame, ..Default::default() };
        };
        // A dead player has no character until they respawn: they're still in the world,
        // where they last were.
        let Some(pawn) = local.pawn.filter(|&p| cx.location(p).is_some()) else {
            return match &self.last {
                Some(last) => WorldSnapshot { frame, dying: true, ..last.clone() },
                None => WorldSnapshot { frame, ..Default::default() },
            };
        };
        let Some(position) = cx.location(pawn) else { return WorldSnapshot { frame, ..Default::default() } };
        let forward = cx.rotation(local.controller).or_else(|| cx.rotation(pawn)).map(|r| dir_to_proto(FRotator { pitch: 0.0, ..r }.forward()));
        self.land(cx, pawn);
        let others = self.read_others(cx, local.world, Some(pawn));
        self.other_pawns = others.iter().map(|(_, p)| *p).collect();
        self.others = others.into_iter().map(|(o, _)| o).collect();
        let here = to_proto(position);
        let game_state = cx.ue.get_obj(local.world, &self.m.game_state);
        let player_state = cx.ue.get_obj(local.controller, &self.m.player_state);
        let player_name = player_state.map(|s| cx.ue.get::<unreal_runtime::FString>(s, &self.m.player_name).to_string_lossy());
        let (world, session) = match (&self.m.world, game_state) {
            (Some(w), Some(state)) => w.read(cx, state, pawn, self.others.len() + 1, player_name.clone()),
            _ => (None, None),
        };
        if self.slow.at.is_none_or(|t| t.elapsed() >= SLOW_REFRESH) {
            self.slow.at = Some(Instant::now());
            if let Some(state) = game_state {
                if let Some(c) = &self.m.creatures {
                    let players: Vec<Obj> = std::iter::once(pawn).chain(self.other_pawns.iter().copied()).collect();
                    let buggy = self.creature_reader.buggy(cx, c, pawn);
                    // The buggy is listed as a companion, not among the bugs.
                    let pet = buggy.as_ref().map(|(_, pet)| *pet);
                    self.slow.creatures = self.creature_reader.nearby(cx, c, state, pawn, &players, here, pet);
                    self.slow.buggy = buggy.map(|(status, _)| status);
                }
                if let Some(m) = &self.m.markers {
                    let read = self.marker_reader.read(cx, m, state, player_state, player_name.as_deref());
                    self.slow.markers = read.markers;
                    self.slow.backpack = read.backpack;
                    let guid = player_state.zip(m.player_guid.as_ref()).map(|(s, p)| cx.ue.get::<[u32; 4]>(s, p));
                    if self.slow.backpack.is_none() && self.slow.bag_scan.is_none_or(|t| t.elapsed() >= BACKPACK_REFRESH) {
                        self.slow.bag_scan = Some(Instant::now());
                        if let (Some(c), Some(g)) = (&self.m.creatures, guid) {
                            self.slow.bag = self.creature_reader.backpack(cx, c, state, g);
                        }
                    }
                    self.slow.backpack = self.slow.backpack.or(self.slow.bag);
                }
                self.slow.environment = self.m.world.as_ref().and_then(|w| w.environment(cx, state));
            }
        }
        let dying = self.m.is_dead.as_ref().and_then(|f| cx.call_get::<bool>(pawn, f)).unwrap_or(false);
        // The app selects companions by their marker, and finds their card by its name.
        let mut markers = self.slow.markers.clone();
        markers.extend(self.slow.buggy.as_ref().and_then(|b| {
            Some(Marker { id: BUGGY.into(), kind: MarkerKind::Companion, label: Some(b.name.clone()), icon: None, position: b.position? })
        }));
        let snapshot = WorldSnapshot {
            frame,
            local: Some(LocalPlayer { position: here, forward, layer: WorldLayer::Surface, camera: None }),
            others: self.others.clone(),
            markers,
            creatures: self.slow.creatures.clone(),
            session,
            world,
            companions: self.slow.buggy.clone().into_iter().collect(),
            environment: self.slow.environment.clone(),
            dropped_bag: self.slow.backpack,
            dying,
            ..Default::default()
        };
        self.last = Some(snapshot.clone());
        snapshot
    }

    unsafe fn find_path(&mut self, cx: &UeCx, from: Vec3, to: Vec3) -> Result<PathInfo, String> {
        // Pins and map clicks can be a little above or below the ground, off the navigation
        // mesh: try again from the ground there.
        UnrealEngine::find_path(cx, from, to).or_else(|e| {
            let ground = Self::ground(cx, to).ok_or(e)?;
            UnrealEngine::find_path(cx, from, Vec3::new(to.x, ground, to.z))
        })
    }

    unsafe fn companion_orders(&mut self, _cx: &UeCx, companion: &str) -> Result<Vec<CompanionOrderOption>, String> {
        let m = self.m.creatures.as_ref().filter(|_| companion == BUGGY).ok_or("unknown companion")?;
        Ok(Creatures::orders(m))
    }

    unsafe fn companion_order(&mut self, cx: &UeCx, companion: &str, order: &str) -> Result<(), String> {
        let m = self.m.creatures.as_ref().filter(|_| companion == BUGGY).ok_or("unknown companion")?;
        let pawn = cx.local().and_then(|l| l.pawn).ok_or("not in a world")?;
        Creatures::order(cx, m, pawn, order)
    }

    unsafe fn game_ping(&mut self, cx: &UeCx, position: Vec3) -> Result<(), String> {
        let m = self.m.markers.as_ref().ok_or("this game version has no waypoints")?;
        let controller = cx.local().map(|l| l.controller).ok_or("not in a world")?;
        Markers::ping(cx, m, controller, position)
    }

    unsafe fn input_blocked(&mut self, cx: &UeCx) -> bool {
        match &self.m.markers {
            Some(m) => self.marker_reader.chat_open(cx, m),
            None => false,
        }
    }

    unsafe fn begin_export(&mut self, cx: &UeCx) -> Result<(), String> {
        cx.local().and_then(|l| l.pawn).ok_or("load into a world first")?;
        self.export = Some(export::export(cx));
        Ok(())
    }

    unsafe fn poll_export(&mut self, _cx: &UeCx) -> Option<Result<WorldExport, String>> {
        self.export.take()
    }

    unsafe fn teleport(&mut self, cx: &UeCx, target: &TeleportTarget) -> Result<(), String> {
        let pawn = cx.local().and_then(|l| l.pawn).ok_or("not in a world")?;
        match target {
            TeleportTarget::Position { position, snap_to_ground: true, .. } => {
                if let Some(y) = Self::ground(cx, *position) {
                    self.landing = None;
                    return self.move_pawn(cx, pawn, Vec3::new(position.x, y + GROUND_CLEARANCE, position.z));
                }
                // That part of the world isn't loaded: wait there, held in the air, until it is.
                let here = cx.location(pawn).map(to_proto).ok_or("the player has no position")?;
                self.set_movement_mode(cx, pawn, MOVE_FLYING)?;
                if let Err(e) = self.move_pawn(cx, pawn, Vec3::new(position.x, here.y, position.z)) {
                    let _ = self.set_movement_mode(cx, pawn, MOVE_WALKING);
                    return Err(e);
                }
                self.landing = Some(Landing { at: *position, from: here, until: Instant::now() + LANDING_WAIT });
                Ok(())
            }
            TeleportTarget::Position { position, .. } => {
                self.landing = None;
                self.move_pawn(cx, pawn, Vec3::new(position.x, position.y + GROUND_CLEARANCE, position.z))
            }
            TeleportTarget::Player { id } => {
                let p = self.others.iter().find(|o| &o.id == id).map(|o| o.position).ok_or("player not found")?;
                self.landing = None;
                self.move_pawn(cx, pawn, Vec3::new(p.x + 1.0, p.y + GROUND_CLEARANCE, p.z))
            }
        }
    }
}

