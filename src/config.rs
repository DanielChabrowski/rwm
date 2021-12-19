use crate::keybind::Keybind;

#[derive(Debug, Default)]
pub struct Config {
    pub keybinds: Vec<Keybind>,
}

impl Config {
    pub fn add_keybind(&mut self, keybind: Keybind) {
        self.keybinds.push(keybind);
    }
}
