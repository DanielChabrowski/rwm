use log::{debug, error, info, trace};
use std::collections::HashMap;
use x::EventMask;
use xcb::{
    x,
    xkb::{EventType, MapPart},
    Xid,
};
use xkbcommon::xkb;

mod keyboard;
use keyboard::Keyboard;

#[derive(Debug)]
struct Client {
    window: x::Window,
}

struct App {
    conn: xcb::Connection,
    root: x::Window,

    keyboard: Keyboard,

    clients: HashMap<u32, Client>,
}

fn register_for_xcb_events(conn: &xcb::Connection, root: x::Window) -> xcb::ProtocolResult<()> {
    let event_mask: xcb::x::EventMask = EventMask::SUBSTRUCTURE_REDIRECT
        | EventMask::STRUCTURE_NOTIFY
        | EventMask::SUBSTRUCTURE_NOTIFY
        | EventMask::PROPERTY_CHANGE
        | EventMask::BUTTON_PRESS
        | EventMask::BUTTON_RELEASE
        | EventMask::POINTER_MOTION
        | EventMask::FOCUS_CHANGE
        | EventMask::ENTER_WINDOW
        | EventMask::LEAVE_WINDOW
        | EventMask::KEY_PRESS;

    let req = xcb::x::ChangeWindowAttributes {
        window: root,
        value_list: &[xcb::x::Cw::EventMask(event_mask.bits())],
    };

    let cookie = conn.send_request_checked(&req);

    conn.check_request(cookie)
}

fn register_for_xkb_events(conn: &xcb::Connection) -> xcb::ProtocolResult<()> {
    let map_parts = MapPart::KEY_TYPES
        | MapPart::KEY_SYMS
        | MapPart::MODIFIER_MAP
        | MapPart::EXPLICIT_COMPONENTS
        | MapPart::KEY_ACTIONS
        | MapPart::KEY_BEHAVIORS
        | MapPart::VIRTUAL_MODS
        | MapPart::VIRTUAL_MOD_MAP;

    let events = EventType::NEW_KEYBOARD_NOTIFY | EventType::MAP_NOTIFY | EventType::STATE_NOTIFY;

    let cookie = conn.send_request_checked(&xcb::xkb::SelectEvents {
        device_spec: xcb::xkb::Id::UseCoreKbd as xcb::xkb::DeviceSpec,
        affect_which: events,
        clear: EventType::empty(),
        select_all: events,
        affect_map: map_parts,
        map: map_parts,
        details: &[],
    });

    conn.check_request(cookie)
}

impl App {
    fn new() -> Self {
        let (conn, screen_num) =
            xcb::Connection::connect_with_extensions(None, &[xcb::Extension::Xkb], &[])
                .expect("Could not make a xcb connection");

        let setup = conn.get_setup();
        let screen = setup.roots().nth(screen_num as usize).unwrap();

        let root: x::Window = screen.root();

        debug!("Root window: {:?}", root);

        register_for_xcb_events(&conn, root)
            .expect("Failed to register for XCB events. Other window manager running?");

        keyboard::setup_xkb_extension(&conn);
        register_for_xkb_events(&conn).expect("Failed to register for XKB events");

        let keyboard = Keyboard::new(&conn);

        Self {
            conn,
            root,
            keyboard,
            clients: HashMap::new(),
        }
    }

    fn run(&mut self) {
        self.grab_keybinds();

        loop {
            let event = self.conn.wait_for_event();
            match event {
                Ok(event) => {
                    self.handle_xcb_event(event);
                }
                Err(e) => {
                    error!("Error while waiting for an event: {:?}", e);
                    break;
                }
            }
        }
    }

    fn handle_xcb_event(&mut self, event: xcb::Event) {
        match event {
            xcb::Event::X(event) => {
                self.handle_x11_event(event);
            }
            xcb::Event::Xkb(event) => {
                self.handle_xkb_event(event);
            }
        }
    }

    fn handle_x11_event(&mut self, event: xcb::x::Event) {
        use xcb::x::Event;

        match event {
            Event::ConfigureRequest(event) => {
                let cookie = self.conn.send_request_checked(&xcb::x::ConfigureWindow {
                    window: event.window(),
                    value_list: &[
                        x::ConfigWindow::X(event.x().into()),
                        x::ConfigWindow::Y(event.y().into()),
                        x::ConfigWindow::Width(event.width().into()),
                        x::ConfigWindow::Height(event.height().into()),
                        x::ConfigWindow::BorderWidth(event.border_width().into()),
                        // x::ConfigWindow::Sibling(event.sibling()),
                        x::ConfigWindow::StackMode(event.stack_mode() as u32),
                    ],
                });

                self.conn.flush().unwrap();

                let result = self.conn.check_request(cookie);
                if result.is_err() {
                    error!("ConfigureRequest failed {:?}", result);
                }
            }
            Event::ConfigureNotify(_) => {}
            Event::DestroyNotify(event) => {
                self.clients.remove(&event.window().resource_id());
            }
            Event::MapRequest(event) => {
                trace!("MapRequest WindowId: {:?}", event.window());

                let cookie = self.conn.send_request_checked(&xcb::x::MapWindow {
                    window: event.window(),
                });

                let result = self.conn.check_request(cookie);
                if result.is_err() {
                    error!("MapRequest failed {:?}", result);
                    return;
                }

                self.clients.insert(
                    event.window().resource_id(),
                    Client {
                        window: event.window(),
                    },
                );
            }
            Event::UnmapNotify(_event) => {
                // Hide the window
            }
            Event::KeyPress(event) => {
                let keycode = event.detail().into();

                let mod4_index = self.keyboard.get_mod_index(xkb::MOD_NAME_LOGO);

                trace!(
                    "Key pressed (code: {}, sym: {:?}, utf-8: {:?}, mods: {:?}, {:?})",
                    event.detail(),
                    self.keyboard.keycode_to_keysym(keycode),
                    self.keyboard.keycode_to_utf8(keycode),
                    self.keyboard
                        .is_mod_active(mod4_index, xkb::STATE_MODS_DEPRESSED),
                    self.keyboard
                        .is_mod_active(mod4_index, xkb::STATE_MODS_EFFECTIVE)
                );

                let rofi_key = self
                    .keyboard
                    .keysym_to_keycode(&self.conn, xkb::keysyms::KEY_D)
                    .unwrap();

                let modifier = self
                    .keyboard
                    .is_mod_active(mod4_index, xkb::STATE_MODS_EFFECTIVE);

                if keycode == rofi_key && modifier {
                    let _ = std::process::Command::new("rofi")
                        .arg("-show")
                        .arg("run")
                        .spawn();
                }
            }
            Event::MotionNotify(_) => {
                // We don't want moves to be logged...
            }
            Event::MappingNotify(e) => {
                error!("Keyboard mapping changed? {:?}", e);
                panic!("Should we handle this?");
            }
            e => {
                trace!("Unhandled event: {:?}", e);
            }
        }
    }

    fn handle_xkb_event(&mut self, event: xcb::xkb::Event) {
        use xcb::xkb::Event;

        match event {
            Event::NewKeyboardNotify(event) => {
                if event.changed().contains(xcb::xkb::NknDetail::KEYCODES) {
                    debug!("xkb NewKeyboardNotifyEvent");
                    self.keyboard.update_keymaps(&self.conn);
                }
            }
            Event::MapNotify(_) => {
                debug!("xkb MapNotifyEvent");
                self.keyboard.update_keymaps(&self.conn);
            }
            Event::StateNotify(event) => {
                self.keyboard.update_state(event);
            }
            _ => {
                trace!("Unsupported xkb event type: {:?}", event);
            }
        }
    }

    fn grab_keybinds(&mut self) {
        let numlock_index = self.keyboard.get_mod_index(xkb::MOD_NAME_NUM);
        let numlock_mask = xcb::x::ModMask::from_bits(1 << numlock_index)
            .expect("Expected a valid numlock modmask");

        let bind = xcb::x::ModMask::N4;
        for modifiers in [
            bind,
            bind | numlock_mask,
            bind | xcb::x::ModMask::LOCK,
            bind | xcb::x::ModMask::LOCK | numlock_mask,
        ] {
            let cookie = self.conn.send_request_checked(&xcb::x::GrabKey {
                owner_events: true,
                grab_window: self.root,
                modifiers,
                key: self
                    .keyboard
                    .keysym_to_keycode(&self.conn, xkb::KEY_d)
                    .unwrap() as u8,
                pointer_mode: xcb::x::GrabMode::Async,
                keyboard_mode: xcb::x::GrabMode::Async,
            });

            self.conn.check_request(cookie).expect("key grabbed");
        }

        self.conn.flush().expect("Flushed");
    }
}

fn main() {
    env_logger::init();
    info!("Welcome to {}", env!("CARGO_PKG_NAME"));

    let mut app = App::new();
    app.run();
}
