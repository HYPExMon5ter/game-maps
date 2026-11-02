//! Hotkey edge detection, polled once per game frame while the game window is focused.
//! The key is never consumed, so the game still sees it (e.g. M still opens the GPS).

use game_maps_protocol::{Hotkey, HotkeyAction};

const VK_SHIFT: u16 = 0x10;
const VK_CONTROL: u16 = 0x11;
const VK_MENU: u16 = 0x12;

/// A key's state as seen by one poll.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KeyState {
    /// Held right now.
    pub down: bool,
    /// Pressed at some point since the previous poll (catches taps shorter than a frame,
    /// e.g. while the game renders slowly right after regaining focus).
    pub tapped: bool,
}

#[derive(Default)]
pub struct Hotkeys {
    bindings: Vec<Hotkey>,
    /// Whether each binding's key was down on the previous poll.
    was_down: Vec<bool>,
}

impl Hotkeys {
    pub fn set(&mut self, bindings: Vec<Hotkey>) {
        self.was_down = vec![true; bindings.len()]; // require a fresh press after rebinding
        self.bindings = bindings;
    }

    /// Returns actions whose key was pressed (up -> down, or tapped) since the last
    /// poll. `key(vk)` reports a key's state.
    pub fn poll(&mut self, mut key: impl FnMut(u16) -> KeyState) -> Vec<HotkeyAction> {
        let mut fired = Vec::new();
        for (i, b) in self.bindings.iter().enumerate() {
            let k = key(b.vk);
            let pressed = (k.down && !self.was_down[i]) || (k.tapped && !k.down);
            let mut held = |vk| key(vk).down;
            if pressed && held(VK_CONTROL) == b.ctrl && held(VK_SHIFT) == b.shift && held(VK_MENU) == b.alt {
                fired.push(b.action);
            }
            self.was_down[i] = k.down;
        }
        fired
    }

    /// Keys of all bindings, so callers can discard input that happened while the
    /// game wasn't focused.
    pub fn keys(&self) -> impl Iterator<Item = u16> + '_ {
        self.bindings.iter().map(|b| b.vk)
    }

    /// Marks every key as held, so keys already down when input resumes don't fire.
    pub fn suppress(&mut self) {
        self.was_down.iter_mut().for_each(|d| *d = true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const M: u16 = 0x4D;

    fn down(d: bool) -> KeyState {
        KeyState { down: d, tapped: d }
    }

    fn m_binding() -> Vec<Hotkey> {
        vec![Hotkey { action: HotkeyAction::ToggleMap, vk: M, ctrl: false, shift: false, alt: false }]
    }

    #[test]
    fn fires_once_per_press() {
        let mut h = Hotkeys::default();
        h.set(m_binding());
        assert!(h.poll(|_| KeyState::default()).is_empty());
        assert_eq!(h.poll(|vk| down(vk == M)), vec![HotkeyAction::ToggleMap]);
        assert!(h.poll(|vk| down(vk == M)).is_empty(), "held key must not repeat");
        assert!(h.poll(|_| KeyState::default()).is_empty());
        assert_eq!(h.poll(|vk| down(vk == M)), vec![HotkeyAction::ToggleMap]);
    }

    #[test]
    fn modifiers_must_match() {
        let mut h = Hotkeys::default();
        h.set(m_binding());
        h.poll(|_| KeyState::default());
        assert!(h.poll(|vk| down(vk == M || vk == VK_CONTROL)).is_empty());
    }

    #[test]
    fn key_held_during_rebind_or_suppression_does_not_fire() {
        let mut h = Hotkeys::default();
        h.set(m_binding());
        assert!(h.poll(|vk| down(vk == M)).is_empty());
        h.poll(|_| KeyState::default());
        h.suppress();
        assert!(h.poll(|vk| down(vk == M)).is_empty());
    }

    #[test]
    fn tap_between_polls_fires_once() {
        let mut h = Hotkeys::default();
        h.set(m_binding());
        h.poll(|_| KeyState::default());
        let tap = |vk| KeyState { down: false, tapped: vk == M };
        assert_eq!(h.poll(tap), vec![HotkeyAction::ToggleMap]);
        assert!(h.poll(|_| KeyState::default()).is_empty());
    }
}
