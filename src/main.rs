use log::{debug, error, info, trace};
use x::EventMask;
use xcb::{
    x,
    xkb::{EventType, MapPart},
};
use xkbcommon::xkb;

mod keyboard;
use keyboard::Keyboard;

struct App {
    conn: xcb::Connection,
    root: x::Window,

    keyboard: Keyboard,
}

fn register_for_xcb_events(conn: &xcb::Connection, root: x::Window) {
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
        .expect("Failed to register for XCB events. Other window manager running?");
}

fn register_for_xkb_events(conn: &xcb::Connection) {
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
        .expect("Failed to register for XKB events");
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

        register_for_xcb_events(&conn, root);

        keyboard::setup_xkb_extension(&conn);
        register_for_xkb_events(&conn);

        let keyboard = Keyboard::new(&conn);

        Self {
            conn,
            root,
            keyboard,
        }
    }

    fn run(&mut self) {
        loop {
            let event = self.conn.wait_for_event();
            match event {
                Ok(event) => {
                    self.handle_xcb_event(event);
                }
                Err(_) => {
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
                trace!("{:?}", event);

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
            Event::MapRequest(event) => {
                trace!("MapRequest WindowId: {:?}", event.window());

                let cookie = self.conn.send_request_checked(&xcb::x::MapWindow {
                    window: event.window(),
                });

                let result = self.conn.check_request(cookie);
                if result.is_err() {
                    error!("MapRequest failed {:?}", result);
                }
            }
            Event::KeyPress(event) => {
                let keycode: xkb::Keycode = event.detail().into();
                let keysym = self.keyboard.keycode_to_keysym(keycode);

                let mod4_index = self.keyboard.get_mod_index("Mod4");

                trace!(
                    "Key pressed (code: {}, sym: {:?}, utf-8: {:?}, mods: {:?}, {:?})",
                    event.detail(),
                    keysym,
                    self.keyboard.keycode_to_utf8(keycode),
                    self.keyboard
                        .is_mod_active(mod4_index, xkb::STATE_MODS_DEPRESSED),
                    self.keyboard
                        .is_mod_active(mod4_index, xkb::STATE_MODS_EFFECTIVE)
                );

                let rofi_key: xkb::Keysym = xkb::keysyms::KEY_d;
                let modifier = self
                    .keyboard
                    .is_mod_active(mod4_index, xkb::STATE_MODS_EFFECTIVE);

                if keysym == rofi_key && modifier {
                    let _ = std::process::Command::new("rofi")
                        .arg("-show")
                        .arg("run")
                        .spawn();
                }
            }
            _ => {}
        }
    }

    fn handle_xkb_event(&mut self, event: xcb::xkb::Event) {
        use xcb::xkb::Event;

        match event {
            Event::NewKeyboardNotify(event) => {
                if event.changed().contains(xcb::xkb::NknDetail::KEYCODES) {
                    self.keyboard.update_keymaps(&self.conn);
                }
            }
            Event::MapNotify(_) => {
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
}

fn main() {
    env_logger::init();
    info!("Welcome to {}", env!("CARGO_PKG_NAME"));

    let mut app = App::new();
    app.run();
}
