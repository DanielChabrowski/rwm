use log::debug;
use std::borrow::Borrow;
use xkbcommon::xkb;

pub fn setup_xkb_extension(conn: &xcb::Connection) {
    let mut major_xkb = 0u16;
    let mut minor_xkb = 0u16;
    let mut base_event = 0u8;
    let mut base_error = 0u8;

    xkb::x11::setup_xkb_extension(
        &conn,
        1,
        0,
        xkb::x11::SetupXkbExtensionFlags::NoFlags,
        &mut major_xkb,
        &mut minor_xkb,
        &mut base_event,
        &mut base_error,
    );
}

fn create_new_xkb_state(
    conn: &xcb::Connection,
    context: &xkb::Context,
    device_id: i32,
) -> (xkb::Keymap, xkb::State) {
    let keymap =
        xkb::x11::keymap_new_from_device(&context, &conn, device_id, xkb::KEYMAP_COMPILE_NO_FLAGS);

    let state = xkb::x11::state_new_from_device(&keymap, &conn, device_id);

    (keymap, state)
}

pub struct Keyboard {
    xkb_context: xkb::Context,
    xkb_device_id: i32,
    xkb_keymap: xkb::Keymap,
    xkb_state: xkb::State,
}

impl Keyboard {
    pub fn new(conn: &xcb::Connection) -> Self {
        let xkb_device_id = xkb::x11::get_core_keyboard_device_id(&conn);
        let xkb_context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);

        let (xkb_keymap, xkb_state) = create_new_xkb_state(conn, &xkb_context, xkb_device_id);

        return Self {
            xkb_device_id,
            xkb_context,
            xkb_keymap,
            xkb_state,
        };
    }

    pub fn update_state(&mut self, event: xcb::xkb::StateNotifyEvent) {
        self.xkb_state.update_mask(
            event.base_mods().bits(),
            event.latched_mods().bits(),
            event.locked_mods().bits(),
            event.base_group() as u32,
            event.latched_group() as u32,
            event.locked_group() as u32,
        );
    }

    pub fn update_keymaps(&mut self, conn: &xcb::Connection) {
        let (xkb_keymap, xkb_state) =
            create_new_xkb_state(conn, &self.xkb_context, self.xkb_device_id);
        self.xkb_keymap = xkb_keymap;
        self.xkb_state = xkb_state;
    }

    pub fn keysym_to_keycodes(
        &mut self,
        conn: &xcb::Connection,
        keysym: xkb::Keysym,
    ) -> Vec<xcb::x::Keycode> {
        let setup = conn.get_setup();
        let min_keycode = setup.min_keycode();
        let max_keycode = setup.max_keycode();

        let cookie = conn.send_request(&xcb::x::GetKeyboardMapping {
            first_keycode: min_keycode,
            count: max_keycode - min_keycode + 1,
        });

        let reply: xcb::x::GetKeyboardMappingReply = conn.wait_for_reply(cookie).unwrap().into();
        let per = reply.keysyms_per_keycode() as usize;
        let keysyms = reply.keysyms();

        let mut keycodes = Vec::new();

        for col in 0..per {
            for keycode in min_keycode..=max_keycode {
                let keysym_group = (keycode - min_keycode) as usize * per;

                match keysyms.get(keysym_group + col) {
                    Some(ks) if *ks == keysym => {
                        debug!(
                            "keysym: {:?}, keycode: {:?}, col: {:?} {:?}",
                            keysym,
                            keycode,
                            col,
                            self.xkb_keymap.mod_get_name(col as u32)
                        );
                        keycodes.push(keycode);
                    }
                    _ => {}
                }
            }
        }

        keycodes.dedup();
        keycodes
    }

    pub fn get_mod_index<S: Borrow<str> + ?Sized>(&self, name: &S) -> u32 {
        self.xkb_keymap.mod_get_index(name)
    }

    pub fn keycode_to_keysym(&self, keycode: xkb::Keycode) -> xkb::Keysym {
        self.xkb_state.key_get_one_sym(keycode)
    }

    pub fn keycode_to_utf8(&self, keycode: xkb::Keycode) -> String {
        self.xkb_state.key_get_utf8(keycode)
    }
}
