use log::{error, info, trace, warn};
use xcb::xproto;
use xkb;
use xkbcommon_sys;

struct App {
    conn: xcb::Connection,
    root: xcb::Window,

    xkb_extension_data: XkbExtensionData,
    xkb_context: xkb::Context,
    xkb_keymap: xkb::Keymap,
    xkb_state: xkb::State,
}

#[derive(Debug)]
struct XkbExtensionData {
    major: u16,
    minor: u16,
    base_event: u8,
    base_error: u8,
}

fn setup_xkb_extension(conn: &xcb::Connection) -> XkbExtensionData {
    conn.prefetch_extension_data(xcb::xkb::id());

    let (base_event, base_error) = match conn.get_extension_data(xcb::xkb::id()) {
        Some(r) => (r.first_event(), r.first_error()),
        None => {
            panic!("XKB extension not supported");
        }
    };

    {
        let cookie = xcb::xkb::use_extension(
            &conn,
            xkb::x11::MIN_MAJOR_XKB_VERSION,
            xkb::x11::MIN_MINOR_XKB_VERSION,
        );

        match cookie.get_reply() {
            Ok(r) => {
                let major = r.server_major();
                let minor = r.server_minor();

                if r.supported() {
                    XkbExtensionData {
                        major,
                        minor,
                        base_event,
                        base_error,
                    }
                } else {
                    panic!(
                        "Requested XKB version not supported by server. Requested: ({}, {}), server: ({}, {})",
                        xkb::x11::MIN_MAJOR_XKB_VERSION,
                        xkb::x11::MIN_MINOR_XKB_VERSION,
						major,
						minor,
                    );
                }
            }
            Err(_) => {
                panic!("Failure during XKB use_extension");
            }
        }
    }
}

fn register_for_xcb_events(conn: &xcb::Connection, root: xcb::Window) {
    let cookie = xproto::change_window_attributes_checked(
        &conn,
        root,
        &[(
            xcb::CW_EVENT_MASK,
            xproto::EVENT_MASK_SUBSTRUCTURE_REDIRECT
                | xproto::EVENT_MASK_STRUCTURE_NOTIFY
                | xproto::EVENT_MASK_SUBSTRUCTURE_NOTIFY
                | xproto::EVENT_MASK_PROPERTY_CHANGE
                | xproto::EVENT_MASK_BUTTON_PRESS
                | xproto::EVENT_MASK_BUTTON_RELEASE
                | xproto::EVENT_MASK_POINTER_MOTION
                | xproto::EVENT_MASK_FOCUS_CHANGE
                | xproto::EVENT_MASK_ENTER_WINDOW
                | xproto::EVENT_MASK_LEAVE_WINDOW
                | xproto::EVENT_MASK_KEY_PRESS,
        )],
    );

    cookie
        .request_check()
        .expect("Failed to register for XCB events. Other window manager running?");
}

fn register_for_xkb_events(conn: &xcb::Connection) {
    let map_parts = xcb::xkb::MAP_PART_KEY_TYPES
        | xcb::xkb::MAP_PART_KEY_SYMS
        | xcb::xkb::MAP_PART_MODIFIER_MAP
        | xcb::xkb::MAP_PART_EXPLICIT_COMPONENTS
        | xcb::xkb::MAP_PART_KEY_ACTIONS
        | xcb::xkb::MAP_PART_KEY_BEHAVIORS
        | xcb::xkb::MAP_PART_VIRTUAL_MODS
        | xcb::xkb::MAP_PART_VIRTUAL_MOD_MAP;

    let events = xcb::xkb::EVENT_TYPE_NEW_KEYBOARD_NOTIFY
        | xcb::xkb::EVENT_TYPE_MAP_NOTIFY
        | xcb::xkb::EVENT_TYPE_STATE_NOTIFY;

    let cookie = xcb::xkb::select_events_checked(
        &conn,
        xcb::xkb::ID_USE_CORE_KBD as u16,
        events as u16,
        0,
        events as u16,
        map_parts as u16,
        map_parts as u16,
        None,
    );

    cookie
        .request_check()
        .expect("Failed to register for XKB events");
}

impl App {
    fn new() -> Self {
        let (conn, screen_num) =
            xcb::Connection::connect(None).expect("Could not make a xcb connection");

        let setup = conn.get_setup();
        let screen: xproto::Screen = setup.roots().nth(screen_num as usize).unwrap();

        let root: xcb::Window = screen.root();

        let xkb_extension_data = setup_xkb_extension(&conn);
        info!("{:?}", xkb_extension_data);

        register_for_xcb_events(&conn, root);
        register_for_xkb_events(&conn);

        let xkb_context = xkb::Context::default();
        let device_id = xkb::x11::device(&conn).expect("Expected core device id");
        let keymap = xkb::x11::keymap(
            &conn,
            device_id,
            &xkb_context,
            xkb::keymap::compile::NO_FLAGS,
        )
        .expect("Expect keymap");

        info!("Keymap modifiers");
        for modifier in keymap.mods().iter() {
            info!("Mod {:?}", modifier);
        }

        let state = xkb::x11::state(&conn, device_id, &keymap).expect("Expect device state");

        Self {
            conn,
            root,
            xkb_extension_data,
            xkb_context,
            xkb_keymap: keymap,
            xkb_state: state,
        }
    }

    fn run(&self) {
        loop {
            let event = self.conn.wait_for_event();
            match event {
                None => {
                    break;
                }
                Some(event) => {
                    self.handle_xcb_event(&event);
                }
            }
        }
    }

    fn handle_xcb_event(&self, event: &xcb::GenericEvent) {
        let r = event.response_type() & !0x80;
        match r {
            xcb::CONFIGURE_REQUEST => {
                let event: &xcb::ConfigureRequestEvent = unsafe { xcb::cast_event(&event) };
                trace!("ConfigureRequest WindowId: {:?}", event.window());

                let value_mask = event.value_mask();
                let values = vec![
                    (xcb::CONFIG_WINDOW_X as u16, event.x() as u32),
                    (xcb::CONFIG_WINDOW_Y as u16, event.y() as u32),
                    (xcb::CONFIG_WINDOW_WIDTH as u16, u32::from(event.width())),
                    (xcb::CONFIG_WINDOW_HEIGHT as u16, u32::from(event.height())),
                    (
                        xcb::CONFIG_WINDOW_BORDER_WIDTH as u16,
                        u32::from(event.border_width()),
                    ),
                    (xcb::CONFIG_WINDOW_SIBLING as u16, event.sibling() as u32),
                    (
                        xcb::CONFIG_WINDOW_STACK_MODE as u16,
                        u32::from(event.stack_mode()),
                    ),
                ];

                let values: Vec<_> = values
                    .into_iter()
                    .filter(|&(mask, _)| mask & value_mask != 0)
                    .collect();

                let cookie = xproto::configure_window_checked(&self.conn, event.window(), &values);

                let result = cookie.request_check();
                if result.is_err() {
                    error!("ConfigureRequest failed {:?}", result);
                }
            }
            xcb::MAP_REQUEST => {
                let event: &xcb::MapRequestEvent = unsafe { xcb::cast_event(&event) };
                trace!("MapRequest WindowId: {:?}", event.window());
                let cookie: xcb::base::VoidCookie<'_> =
                    xproto::map_window_checked(&self.conn, event.window());

                let result = cookie.request_check();
                if result.is_err() {
                    error!("MapRequest failed {:?}", result);
                }
            }
            xcb::KEY_PRESS => {
                let key_press: &xcb::KeyPressEvent = unsafe { xcb::cast_event(&event) };
                let keycode: xproto::Keycode = key_press.detail();
                let keysym = self.xkb_state.key(keycode).sym();

                trace!(
                    "Key pressed (code: {}, sym: {:?}, utf-8: {:?}, mods: {:?}, {:?})",
                    key_press.detail(),
                    keysym,
                    keysym.map(|s| s.utf8()),
                    self.xkb_state
                        .mods()
                        .active(0, xkb::state::Components::MODS_DEPRESSED),
                    self.xkb_state
                        .mods()
                        .active(0, xkb::state::Components::MODS_EFFECTIVE)
                );

                let rofi_key = xkb::Keysym::from(xkbcommon_sys::XKB_KEY_d);
                let modifier = self
                    .xkb_state
                    .mods()
                    .active("Mod4", xkb::state::Components::MODS_EFFECTIVE);
                if let Some(keysym) = keysym {
                    if keysym == rofi_key && modifier {
                        let _ = std::process::Command::new("rofi")
                            .arg("-show")
                            .arg("run")
                            .spawn();
                    }
                }
            }
            xcb::MAP_NOTIFY => trace!("Map notification"),
            xcb::UNMAP_NOTIFY => trace!("Unmap notification"),
            xcb::MOTION_NOTIFY => {}
            xcb::MAPPING_NOTIFY => trace!("Mapping notification"),
            xcb::CONFIGURE_NOTIFY => trace!("Configure notification"),
            xcb::CREATE_NOTIFY => trace!("Create notification"),
            xcb::DESTROY_NOTIFY => trace!("Destroy notification"),
            xcb::PROPERTY_NOTIFY => trace!("Property notification"),
            xcb::ENTER_NOTIFY => trace!("Enter notification"),
            xcb::LEAVE_NOTIFY => trace!("Leave notification"),
            xcb::FOCUS_IN => trace!("Focus in event"),
            xcb::FOCUS_OUT => trace!("Focus out event"),
            xcb::CLIENT_MESSAGE => trace!("Client message"),
            _ => {
                if r == self.xkb_extension_data.base_event {
                    self.handle_xkb_event(event);
                } else {
                    warn!("Unsupported event: {}", r);
                }
            }
        }
    }

    fn handle_xkb_event(&self, event: &xcb::GenericEvent) {
        let xkb_event_type = unsafe { (*event.ptr).pad0 };

        match xkb_event_type {
            xcb::xkb::NEW_KEYBOARD_NOTIFY => {
                trace!("TODO XKB New keyboard notification");
            }
            xcb::xkb::MAP_NOTIFY => {
                trace!("TODO XKB Map notification");
            }
            xcb::xkb::STATE_NOTIFY => {
                trace!("XKB State notification");
                let state_notify: &xcb::xkb::StateNotifyEvent = unsafe { xcb::cast_event(&event) };

                self.xkb_state.clone().update().mask(
                    state_notify.base_mods(),
                    state_notify.latched_mods(),
                    state_notify.locked_mods(),
                    state_notify.base_group(),
                    state_notify.latched_group(),
                    state_notify.locked_group(),
                );
            }
            _ => {
                warn!("Unsupported xkb event type: {}", xkb_event_type);
            }
        }
    }
}

fn main() {
    env_logger::init();
    info!("Welcome to {}", env!("CARGO_PKG_NAME"));

    let app = App::new();
    app.run();
}
