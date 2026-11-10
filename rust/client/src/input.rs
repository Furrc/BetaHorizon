use std::collections::HashSet;
use winit::{event::ElementState, keyboard::KeyCode};

#[derive(Debug, Default)]
pub struct InputState {
    pressed: HashSet<KeyCode>,
    pub mouse_locked: bool,
    pub mouse_delta: (f32, f32),
}

impl InputState {
    pub fn key(&mut self, code: KeyCode, state: ElementState) {
        match state {
            ElementState::Pressed => { self.pressed.insert(code); }
            ElementState::Released => { self.pressed.remove(&code); }
        }
    }
    pub fn pressed(&self, code: KeyCode) -> bool { self.pressed.contains(&code) }
    pub fn end_frame(&mut self) { self.mouse_delta = (0.0, 0.0); }
}
