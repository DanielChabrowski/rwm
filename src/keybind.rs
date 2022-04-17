use crate::commands::BoxCommand;
use xcb::x::{KeyButMask, Keycode};
use xkbcommon::xkb::Keysym;

#[derive(Debug, PartialEq, Eq)]
pub struct KeySequence {
    keysym: Keysym,
    modifiers: KeyButMask,
}

impl TryFrom<&str> for KeySequence {
    type Error = anyhow::Error;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let mut keysym: Option<Keysym> = None;
        let mut modifiers = KeyButMask::empty();

        for part in value.split('-') {
            if part.len() == 1
                && part
                    .chars()
                    .next()
                    .map_or_else(|| false, |c| c.is_uppercase())
            {
                // Modifiers
                if let Some(modifier) = part.chars().next() {
                    let modifier = match modifier {
                        'S' => Ok(KeyButMask::SHIFT),
                        'C' => Ok(KeyButMask::CONTROL),
                        'M' => Ok(KeyButMask::MOD4),
                        _ => Err(anyhow::anyhow!("Invalid modifier {}", modifier)),
                    };

                    modifiers |= modifier?;
                }
            } else {
                keysym = Some(xkbcommon::xkb::keysym_from_name(
                    part,
                    xkbcommon::xkb::KEYSYM_NO_FLAGS,
                ));

                if keysym == Some(xkbcommon::xkb::KEY_NoSymbol) {
                    anyhow::bail!("Unrecognized keysym: {}", part);
                }
            }
        }

        let keysym = keysym.ok_or_else(|| anyhow::anyhow!("KeySequence is missing a keysym"))?;
        Ok(Self { keysym, modifiers })
    }
}

impl KeySequence {
    pub fn keysym(&self) -> Keysym {
        self.keysym
    }

    pub fn modifiers(&self) -> KeyButMask {
        self.modifiers
    }
}

#[cfg(test)]
mod tests {
    use super::KeySequence;
    use xcb::x::KeyButMask;

    #[test]
    fn try_from_key_sequence() {
        assert_eq!(
            KeySequence::try_from("C-x").unwrap(),
            KeySequence {
                keysym: xkbcommon::xkb::KEY_x,
                modifiers: KeyButMask::CONTROL
            }
        );

        assert_eq!(
            KeySequence::try_from("C-S-s").unwrap(),
            KeySequence {
                keysym: xkbcommon::xkb::KEY_s,
                modifiers: KeyButMask::SHIFT | KeyButMask::CONTROL,
            }
        );

        // Invalid modifier
        assert!(KeySequence::try_from("X-z").is_err());

        // Unknown keysym
        assert!(KeySequence::try_from("C-?").is_err());
    }
}

pub struct Keybind {
    key_sequence: KeySequence,
    keycodes: Vec<Keycode>,
    command: BoxCommand,
}

impl Keybind {
    pub fn new(key_sequence: KeySequence, command: BoxCommand) -> Self {
        Self {
            key_sequence,
            keycodes: Vec::new(),
            command,
        }
    }

    pub fn key_sequence(&self) -> &KeySequence {
        &self.key_sequence
    }

    pub fn update_keycodes(&mut self, keycodes: Vec<Keycode>) {
        self.keycodes = keycodes;
    }

    pub fn command(&self) -> &BoxCommand {
        &self.command
    }

    pub fn matches(&self, keycode: Keycode, modifiers: KeyButMask) -> bool {
        self.keycodes.contains(&keycode) && self.key_sequence.modifiers == modifiers
    }
}

impl std::fmt::Debug for Keybind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Keybind")
            .field("key_sequence", &self.key_sequence)
            .field("keycodes", &self.keycodes)
            .finish()
    }
}
