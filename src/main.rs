use log::{debug, error, info, trace, warn};
use std::collections::HashMap;
use x::EventMask;
use xcb::{
    x::{self, KeyButMask},
    xkb::EventType,
    Xid,
};
use xkbcommon::xkb;

mod keyboard;
use keyboard::Keyboard;

mod commands;
use commands::RofiCommand;

mod config;
use config::Config;

mod keybind;
use keybind::{KeySequence, Keybind};

#[derive(Debug)]
struct Client {
    window: x::Window,
}

struct App {
    conn: xcb::Connection,
    root: x::Window,

    config: Config,

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
        value_list: &[xcb::x::Cw::EventMask(event_mask)],
    };

    let cookie = conn.send_request_checked(&req);

    conn.check_request(cookie)
}

fn register_for_xkb_events(conn: &xcb::Connection) -> xcb::ProtocolResult<()> {
    use xcb::xkb::MapPart;
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

fn register_for_randr_events(conn: &xcb::Connection, root: x::Window) -> xcb::ProtocolResult<()> {
    use xcb::randr::NotifyMask;
    let notify_mask = NotifyMask::SCREEN_CHANGE
        | NotifyMask::CRTC_CHANGE
        | NotifyMask::OUTPUT_CHANGE
        | NotifyMask::OUTPUT_PROPERTY
        | NotifyMask::PROVIDER_CHANGE
        | NotifyMask::PROVIDER_PROPERTY
        | NotifyMask::RESOURCE_CHANGE
        | NotifyMask::LEASE;

    let cookie = conn.send_request_checked(&xcb::randr::SelectInput {
        window: root,
        enable: notify_mask,
    });

    conn.check_request(cookie)
}

fn load_config() -> anyhow::Result<Config> {
    let mut config = Config::default();

    config.add_keybind(Keybind::new(
        KeySequence::try_from("M-d").unwrap(),
        Box::new(RofiCommand),
    ));

    Ok(config)
}

impl App {
    fn new() -> Self {
        let (conn, screen_num) = xcb::Connection::connect_with_extensions(
            None,
            &[xcb::Extension::Xkb, xcb::Extension::RandR],
            &[],
        )
        .expect("XCB connection established");

        let setup = conn.get_setup();
        let screen = setup.roots().nth(screen_num as usize).unwrap();

        let root: x::Window = screen.root();

        debug!(
            "Root window: {:?}, width: {}px, height: {}px",
            root,
            screen.width_in_pixels(),
            screen.height_in_pixels()
        );

        register_for_xcb_events(&conn, root)
            .expect("Failed to register for XCB events. Other window manager running?");

        register_for_randr_events(&conn, root).expect("Failed to register for XrandR events");

        keyboard::setup_xkb_extension(&conn);
        register_for_xkb_events(&conn).expect("Failed to register for XKB events");

        let config = load_config().expect("Config loaded");

        let keyboard = Keyboard::new(&conn);

        Self {
            conn,
            root,
            config,
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
            xcb::Event::RandR(event) => {
                self.handle_xrandr_event(event);
            }
            xcb::Event::Unknown(event) => {
                warn!("Unknown event: {:?}", event);
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
                        x::ConfigWindow::StackMode(event.stack_mode()),
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
                let keycode = event.detail();

                let numlock_index = self.keyboard.get_mod_index(xkb::MOD_NAME_NUM);
                let numlock_mask = KeyButMask::from_bits(1 << numlock_index)
                    .expect("Expected a valid numlock modmask");
                let modmask = event.state() - (KeyButMask::LOCK | numlock_mask);

                trace!(
                    "Key pressed (code: {}, sym: {:?}, utf-8: {:?}, modmask: {:?})",
                    keycode,
                    self.keyboard.keycode_to_keysym(keycode.into()),
                    self.keyboard.keycode_to_utf8(keycode.into()),
                    modmask
                );

                for keybind in &self.config.keybinds {
                    if keybind.matches(keycode, modmask) {
                        keybind
                            .command()
                            .execute()
                            .expect("Keybind command executed");
                        break;
                    }
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
                    self.ungrab_keybinds();
                    self.grab_keybinds();
                }
            }
            Event::MapNotify(_) => {
                debug!("xkb MapNotifyEvent");
                self.keyboard.update_keymaps(&self.conn);
                self.ungrab_keybinds();
                self.grab_keybinds();
            }
            Event::StateNotify(event) => {
                self.keyboard.update_state(event);
            }
            _ => {
                trace!("Unsupported xkb event type: {:?}", event);
            }
        }
    }

    fn handle_xrandr_event(&mut self, event: xcb::randr::Event) {
        use xcb::randr::Event;

        match event {
            Event::ScreenChangeNotify(event) => {
                trace!("{:?}", event);
            }
            Event::Notify(event) => {
                trace!("{:?}", event);
            }
        }
    }

    fn ungrab_keybinds(&self) {
        let cookie = self.conn.send_request_checked(&xcb::x::UngrabKey {
            key: xcb::x::Grab::Any as u8,
            grab_window: self.root,
            modifiers: xcb::x::ModMask::ANY,
        });

        self.conn.check_request(cookie).expect("keys ungrabbed");
        self.conn.flush().expect("Flushed");
    }

    fn grab_keybinds(&mut self) {
        let numlock_index = self.keyboard.get_mod_index(xkb::MOD_NAME_NUM);
        let numlock_mask = xcb::x::ModMask::from_bits(1 << numlock_index)
            .expect("Expected a valid numlock modmask");

        for keybind in &mut self.config.keybinds {
            let mask =
                xcb::x::ModMask::from_bits_truncate(keybind.key_sequence().modifiers().bits());

            let keycodes = self
                .keyboard
                .keysym_to_keycodes(&self.conn, keybind.key_sequence().keysym());

            debug!(
                "Keysym: {:?}, keycodes: {:?}",
                keybind.key_sequence().keysym(),
                keycodes
            );

            for keycode in &keycodes {
                for modifiers in [
                    mask,
                    mask | numlock_mask,
                    mask | xcb::x::ModMask::LOCK,
                    mask | xcb::x::ModMask::LOCK | numlock_mask,
                ] {
                    let cookie = self.conn.send_request_checked(&xcb::x::GrabKey {
                        owner_events: true,
                        grab_window: self.root,
                        modifiers,
                        key: *keycode,
                        pointer_mode: xcb::x::GrabMode::Async,
                        keyboard_mode: xcb::x::GrabMode::Async,
                    });

                    self.conn.check_request(cookie).expect("key grabbed");
                }
            }

            keybind.update_keycodes(keycodes);
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
