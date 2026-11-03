//! Decides whether a new snapshot is worth sending. Keeps pipe traffic and app-side
//! rendering proportional to actual movement instead of frame rate.

use game_maps_protocol::{CameraView, Vec3, WorldSnapshot};

/// Minimum movement (world units) before position updates are sent.
const LOCAL_MOVE_EPSILON: f32 = 0.02;
const REMOTE_MOVE_EPSILON: f32 = 0.05;
/// Markers (companions, vehicles) are drawn as icons; sub-meter moves don't matter.
const MARKER_MOVE_EPSILON: f32 = 0.5;
/// Creatures are dots on the map: a meter of movement is enough to redraw.
const CREATURE_MOVE_EPSILON: f32 = 1.0;
/// cos(~1.8°): smaller heading changes are not worth a message.
const FORWARD_DOT_THRESHOLD: f32 = 0.9995;

pub fn has_meaningful_change(prev: &WorldSnapshot, next: &WorldSnapshot) -> bool {
    match (&prev.local, &next.local) {
        (None, None) => {}
        (Some(a), Some(b)) => {
            if a.layer != b.layer
                || a.position.distance_sq(b.position) > LOCAL_MOVE_EPSILON * LOCAL_MOVE_EPSILON
                || forward_changed(a.forward, b.forward)
                || camera_changed(a.camera.as_ref(), b.camera.as_ref())
            {
                return true;
            }
        }
        _ => return true,
    }
    if prev.others.len() != next.others.len() || prev.markers.len() != next.markers.len() || prev.creatures.len() != next.creatures.len() {
        return true;
    }
    let creatures_changed = prev
        .creatures
        .iter()
        .zip(&next.creatures)
        .any(|(a, b)| a.id != b.id || a.kind != b.kind || a.position.distance_sq(b.position) > CREATURE_MOVE_EPSILON * CREATURE_MOVE_EPSILON);
    if creatures_changed || prev.session != next.session {
        return true;
    }
    let markers_changed = prev.markers.iter().zip(&next.markers).any(|(a, b)| {
        a.id != b.id
            || a.label != b.label
            || a.icon != b.icon
            || a.position.distance_sq(b.position) > MARKER_MOVE_EPSILON * MARKER_MOVE_EPSILON
    });
    markers_changed
        || prev.others.iter().zip(&next.others).any(|(a, b)| {
        a.id != b.id
            || a.name != b.name
            || a.color != b.color
            || a.position.distance_sq(b.position) > REMOTE_MOVE_EPSILON * REMOTE_MOVE_EPSILON
            || forward_changed(a.forward, b.forward)
        })
}

/// Camera moves or turns matter for waypoints drawn over the game (pitch included,
/// which the player's heading doesn't capture).
fn camera_changed(a: Option<&CameraView>, b: Option<&CameraView>) -> bool {
    match (a, b) {
        (None, None) => false,
        (Some(a), Some(b)) => {
            a.position.distance_sq(b.position) > LOCAL_MOVE_EPSILON * LOCAL_MOVE_EPSILON
                || forward_changed(Some(a.forward), Some(b.forward))
                || (a.fov_y - b.fov_y).abs() > 0.5
        }
        _ => true,
    }
}

fn forward_changed(a: Option<Vec3>, b: Option<Vec3>) -> bool {
    match (a, b) {
        (None, None) => false,
        (Some(a), Some(b)) => {
            let la = (a.x * a.x + a.y * a.y + a.z * a.z).sqrt();
            let lb = (b.x * b.x + b.y * b.y + b.z * b.z).sqrt();
            if la == 0.0 || lb == 0.0 {
                return la != lb;
            }
            (a.x * b.x + a.y * b.y + a.z * b.z) / (la * lb) < FORWARD_DOT_THRESHOLD
        }
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use game_maps_protocol::{Creature, CreatureKind, LocalPlayer, RemotePlayer, WorldLayer};

    fn snap(pos: Vec3, fwd: Vec3) -> WorldSnapshot {
        WorldSnapshot {
            frame: 0,
            local: Some(LocalPlayer { position: pos, forward: Some(fwd), layer: WorldLayer::Surface, camera: None }),
            others: vec![],
            markers: vec![],
            creatures: vec![],
            session: None,
        }
    }

    #[test]
    fn creatures_moving_a_meter_are_sent() {
        let a = snap(Vec3::new(1.0, 1.0, 1.0), Vec3::new(0.0, 0.0, 1.0));
        let mut b = a.clone();
        b.creatures.push(Creature { id: "actor:1".into(), kind: CreatureKind::Animal, name: "Deer".into(), position: Vec3::ZERO });
        assert!(has_meaningful_change(&a, &b));
        let mut c = b.clone();
        c.creatures[0].position.x = 0.5;
        assert!(!has_meaningful_change(&b, &c));
        c.creatures[0].position.x = 1.5;
        assert!(has_meaningful_change(&b, &c));
    }

    #[test]
    fn tiny_jitter_is_ignored() {
        let a = snap(Vec3::new(10.0, 5.0, 10.0), Vec3::new(0.0, 0.0, 1.0));
        let b = snap(Vec3::new(10.01, 5.0, 10.0), Vec3::new(0.0, 0.0, 1.0));
        assert!(!has_meaningful_change(&a, &b));
    }

    #[test]
    fn movement_and_turning_are_sent() {
        let a = snap(Vec3::new(10.0, 5.0, 10.0), Vec3::new(0.0, 0.0, 1.0));
        assert!(has_meaningful_change(&a, &snap(Vec3::new(10.1, 5.0, 10.0), Vec3::new(0.0, 0.0, 1.0))));
        assert!(has_meaningful_change(&a, &snap(Vec3::new(10.0, 5.0, 10.0), Vec3::new(0.1, 0.0, 1.0))));
    }

    #[test]
    fn entering_world_or_layer_change_is_sent() {
        let a = snap(Vec3::new(1.0, 1.0, 1.0), Vec3::new(0.0, 0.0, 1.0));
        assert!(has_meaningful_change(&WorldSnapshot::default(), &a));
        let mut b = a.clone();
        b.local.as_mut().unwrap().layer = WorldLayer::Caves;
        assert!(has_meaningful_change(&a, &b));
    }

    #[test]
    fn remote_player_join_and_move_are_sent() {
        let a = snap(Vec3::new(1.0, 1.0, 1.0), Vec3::new(0.0, 0.0, 1.0));
        let mut b = a.clone();
        b.others.push(RemotePlayer { id: "1".into(), name: "A".into(), color: None, position: Vec3::ZERO, forward: None });
        assert!(has_meaningful_change(&a, &b));
        let mut c = b.clone();
        c.others[0].position.x = 1.0;
        assert!(has_meaningful_change(&b, &c));
        assert!(!has_meaningful_change(&b, &b.clone()));
    }
}
