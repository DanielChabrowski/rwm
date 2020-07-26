use log::{debug, info, trace, warn};
use xcb::xproto;

fn main() {
    env_logger::init();

    info!("Welcome to rwm");

    let (conn, screen_num) = xcb::Connection::connect(None).unwrap();
    let setup = conn.get_setup();
    let screen: xproto::Screen = setup.roots().nth(screen_num as usize).unwrap();

    let root_window = screen.root();
    debug!("Root window: {:?}", root_window);

    let cookie = xproto::change_window_attributes_checked(
        &conn,
        root_window,
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

    let err = cookie.request_check();
    if err.is_err() {
        panic!("failed to select notify events from xcb xkb");
    }

    loop {
        let event = conn.wait_for_event();
        match event {
            None => {
                break;
            }
            Some(event) => {
                let r = event.response_type() & !0x80;
                match r {
                    xcb::CONFIGURE_REQUEST => {
                        let event: &xcb::ConfigureRequestEvent = unsafe { xcb::cast_event(&event) };
                        trace!("ConfigureRequest WindowId: {:?}", event.window());
                        let cookie = xproto::configure_window_checked(&conn, event.window(), &[]);

                        let result = cookie.request_check();
                        if result.is_err() {
                            panic!("ConfigureRequest failed {:?}", result);
                        }
                    }
                    xcb::MAP_REQUEST => {
                        let event: &xcb::MapRequestEvent = unsafe { xcb::cast_event(&event) };
                        trace!("MapRequest WindowId: {:?}", event.window());
                        let cookie: xcb::base::VoidCookie<'_> =
                            xproto::map_window_checked(&conn, event.window());

                        let result = cookie.request_check();
                        if result.is_err() {
                            panic!("MapRequest failed {:?}", result);
                        }
                    }
                    xcb::KEY_PRESS => {
                        let key_press: &xcb::KeyPressEvent = unsafe { xcb::cast_event(&event) };
                        trace!("Key '{}' pressed", key_press.detail());

                        let keycode: xproto::Keycode = key_press.detail();

                        if keycode == 24 {
                            break;
                        } else if key_press.detail() == 41 {
                            let _ = std::process::Command::new("rofi")
                                .arg("-show")
                                .arg("run")
                                .spawn();
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
                        warn!("Unsupported event: {}", r);
                    }
                }
            }
        }
    }
}
