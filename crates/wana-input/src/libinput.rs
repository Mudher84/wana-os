//! libinput on udev: the seat's input devices and their events, converted
//! to owned Rust values.

use crate::ffi;
use crate::keyboard::cstr_or;
use std::ffi::CString;
use std::io;
use std::os::raw::{c_char, c_int, c_void};
use std::os::unix::io::RawFd;
use std::ptr::NonNull;

/// Device capabilities, in libinput's order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    Keyboard,
    Pointer,
    Touch,
    TabletTool,
    TabletPad,
    Gesture,
    Switch,
}

impl Capability {
    pub const ALL: [(Capability, c_int); 7] = [
        (Capability::Keyboard, ffi::LIBINPUT_DEVICE_CAP_KEYBOARD),
        (Capability::Pointer, ffi::LIBINPUT_DEVICE_CAP_POINTER),
        (Capability::Touch, ffi::LIBINPUT_DEVICE_CAP_TOUCH),
        (Capability::TabletTool, ffi::LIBINPUT_DEVICE_CAP_TABLET_TOOL),
        (Capability::TabletPad, ffi::LIBINPUT_DEVICE_CAP_TABLET_PAD),
        (Capability::Gesture, ffi::LIBINPUT_DEVICE_CAP_GESTURE),
        (Capability::Switch, ffi::LIBINPUT_DEVICE_CAP_SWITCH),
    ];

    pub fn name(self) -> &'static str {
        match self {
            Capability::Keyboard => "keyboard",
            Capability::Pointer => "pointer",
            Capability::Touch => "touch",
            Capability::TabletTool => "tablet-tool",
            Capability::TabletPad => "tablet-pad",
            Capability::Gesture => "gesture",
            Capability::Switch => "switch",
        }
    }
}

/// An input device as libinput reports it.
#[derive(Debug, Clone, PartialEq)]
pub struct Device {
    /// Kernel device name, e.g. `AT Translated Set 2 keyboard`.
    pub name: String,
    /// Event node name, e.g. `event2`.
    pub sysname: String,
    pub caps: Vec<Capability>,
}

impl Device {
    pub fn has(&self, cap: Capability) -> bool {
        self.caps.contains(&cap)
    }

    /// `keyboard, pointer`, or `none`.
    pub fn caps_list(&self) -> String {
        if self.caps.is_empty() {
            return "none".into();
        }
        let names: Vec<_> = self.caps.iter().map(|c| c.name()).collect();
        names.join(", ")
    }
}

/// One libinput event, owned, with the event node it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    /// Event node name of the source device, e.g. `event1`.
    pub device: String,
    pub kind: EventKind,
}

/// What happened.
#[derive(Debug, Clone, PartialEq)]
pub enum EventKind {
    DeviceAdded(Device),
    DeviceRemoved(Device),
    /// Keyboard key, as an evdev key code (`KEY_A` = 30).
    Key {
        code: u32,
        pressed: bool,
    },
    /// Relative pointer motion, accelerated, in pixel-like units.
    Motion {
        dx: f64,
        dy: f64,
    },
    /// Absolute pointer position as fractions of the device range (0..1).
    MotionAbsolute {
        x: f64,
        y: f64,
    },
    /// Pointer button, as an evdev code (`BTN_LEFT` = 0x110).
    Button {
        code: u32,
        pressed: bool,
    },
    /// Mouse wheel, in 1/120 detent units (one notch = 120).
    Scroll {
        vertical: f64,
        horizontal: f64,
    },
    /// An event type Wana does not handle yet (touch, tablet, gestures...).
    Other(i32),
}

/// Resolution used to read absolute positions as fractions.
const ABS_SCALE: u32 = 1_000_000;

unsafe extern "C" fn open_restricted(
    path: *const c_char,
    flags: c_int,
    _user_data: *mut c_void,
) -> c_int {
    // SAFETY: libinput passes a valid NUL-terminated path.
    let fd = unsafe { ffi::open(path, flags | ffi::O_CLOEXEC) };
    if fd < 0 {
        -io::Error::last_os_error().raw_os_error().unwrap_or(1)
    } else {
        fd
    }
}

unsafe extern "C" fn close_restricted(fd: c_int, _user_data: *mut c_void) {
    // SAFETY: fd was returned by open_restricted and libinput gives it back once.
    unsafe { ffi::close(fd) };
}

static INTERFACE: ffi::libinput_interface = ffi::libinput_interface {
    open_restricted,
    close_restricted,
};

/// A libinput context bound to one udev seat.
#[derive(Debug)]
pub struct Libinput {
    li: NonNull<ffi::libinput>,
    udev: NonNull<ffi::udev>,
}

impl Libinput {
    /// Creates a context on udev and assigns `seat` (normally `seat0`).
    /// Devices show up as `DeviceAdded` events after the first dispatch.
    pub fn new(seat: &str) -> Result<Libinput, String> {
        let seat_c = CString::new(seat).map_err(|e| format!("seat {seat:?}: {e}"))?;
        // SAFETY: no arguments.
        let udev = NonNull::new(unsafe { ffi::udev_new() }).ok_or("udev_new failed")?;
        // SAFETY: INTERFACE is 'static; udev is valid (libinput takes its own ref).
        let li = unsafe {
            ffi::libinput_udev_create_context(&INTERFACE, std::ptr::null_mut(), udev.as_ptr())
        };
        let Some(li) = NonNull::new(li) else {
            // SAFETY: valid and not used afterwards.
            unsafe { ffi::udev_unref(udev.as_ptr()) };
            return Err("libinput_udev_create_context failed".into());
        };
        let ctx = Libinput { li, udev };
        // SAFETY: li is valid.
        unsafe { ffi::libinput_log_set_priority(li.as_ptr(), ffi::LIBINPUT_LOG_PRIORITY_ERROR) };
        // SAFETY: li is valid, seat_c outlives the call.
        if unsafe { ffi::libinput_udev_assign_seat(li.as_ptr(), seat_c.as_ptr()) } != 0 {
            return Err(format!("libinput_udev_assign_seat({seat}) failed"));
        }
        Ok(ctx)
    }

    /// File descriptor to poll for readability.
    pub fn fd(&self) -> RawFd {
        // SAFETY: li is valid.
        unsafe { ffi::libinput_get_fd(self.li.as_ptr()) }
    }

    /// Reads pending kernel events into libinput's queue.
    pub fn dispatch(&mut self) -> io::Result<()> {
        // SAFETY: li is valid.
        let rc = unsafe { ffi::libinput_dispatch(self.li.as_ptr()) };
        if rc < 0 {
            Err(io::Error::from_raw_os_error(-rc))
        } else {
            Ok(())
        }
    }

    /// Takes the next queued event, if any.
    pub fn next_event(&mut self) -> Option<Event> {
        // SAFETY: li is valid.
        let ev = NonNull::new(unsafe { ffi::libinput_get_event(self.li.as_ptr()) })?;
        // SAFETY: ev is a valid event until destroyed below; the typed
        // accessors are only called for matching event types.
        let event = unsafe { convert(ev.as_ptr()) };
        // SAFETY: ev is valid and destroyed exactly once.
        unsafe { ffi::libinput_event_destroy(ev.as_ptr()) };
        Some(event)
    }
}

impl Drop for Libinput {
    fn drop(&mut self) {
        // SAFETY: both valid, released once; libinput drops its own udev ref.
        unsafe {
            ffi::libinput_unref(self.li.as_ptr());
            ffi::udev_unref(self.udev.as_ptr());
        }
    }
}

/// # Safety
/// `ev` must be a valid libinput event.
unsafe fn convert(ev: *mut ffi::libinput_event) -> Event {
    // SAFETY: `ev` is valid (caller); every event has a device.
    let device = unsafe {
        cstr_or(
            ffi::libinput_device_get_sysname(ffi::libinput_event_get_device(ev)),
            "?",
        )
    };
    // SAFETY: as above.
    let kind = unsafe { convert_kind(ev) };
    Event { device, kind }
}

/// # Safety
/// `ev` must be a valid libinput event.
unsafe fn convert_kind(ev: *mut ffi::libinput_event) -> EventKind {
    // SAFETY: `ev` is valid (caller); each typed accessor is used only for
    // its own event type, so the returned sub-event pointers are valid too.
    unsafe {
        let kind = ffi::libinput_event_get_type(ev);
        match kind {
            ffi::LIBINPUT_EVENT_DEVICE_ADDED => EventKind::DeviceAdded(device(ev)),
            ffi::LIBINPUT_EVENT_DEVICE_REMOVED => EventKind::DeviceRemoved(device(ev)),
            ffi::LIBINPUT_EVENT_KEYBOARD_KEY => {
                let k = ffi::libinput_event_get_keyboard_event(ev);
                EventKind::Key {
                    code: ffi::libinput_event_keyboard_get_key(k),
                    pressed: ffi::libinput_event_keyboard_get_key_state(k)
                        == ffi::LIBINPUT_KEY_STATE_PRESSED,
                }
            }
            ffi::LIBINPUT_EVENT_POINTER_MOTION => {
                let p = ffi::libinput_event_get_pointer_event(ev);
                EventKind::Motion {
                    dx: ffi::libinput_event_pointer_get_dx(p),
                    dy: ffi::libinput_event_pointer_get_dy(p),
                }
            }
            ffi::LIBINPUT_EVENT_POINTER_MOTION_ABSOLUTE => {
                let p = ffi::libinput_event_get_pointer_event(ev);
                let scale = f64::from(ABS_SCALE);
                EventKind::MotionAbsolute {
                    x: ffi::libinput_event_pointer_get_absolute_x_transformed(p, ABS_SCALE) / scale,
                    y: ffi::libinput_event_pointer_get_absolute_y_transformed(p, ABS_SCALE) / scale,
                }
            }
            ffi::LIBINPUT_EVENT_POINTER_BUTTON => {
                let p = ffi::libinput_event_get_pointer_event(ev);
                EventKind::Button {
                    code: ffi::libinput_event_pointer_get_button(p),
                    pressed: ffi::libinput_event_pointer_get_button_state(p)
                        == ffi::LIBINPUT_BUTTON_STATE_PRESSED,
                }
            }
            ffi::LIBINPUT_EVENT_POINTER_SCROLL_WHEEL => {
                let p = ffi::libinput_event_get_pointer_event(ev);
                let axis = |a| {
                    if ffi::libinput_event_pointer_has_axis(p, a) != 0 {
                        ffi::libinput_event_pointer_get_scroll_value_v120(p, a)
                    } else {
                        0.0
                    }
                };
                EventKind::Scroll {
                    vertical: axis(ffi::LIBINPUT_POINTER_AXIS_SCROLL_VERTICAL),
                    horizontal: axis(ffi::LIBINPUT_POINTER_AXIS_SCROLL_HORIZONTAL),
                }
            }
            other => EventKind::Other(other),
        }
    }
}

/// # Safety
/// `ev` must be a valid libinput event.
unsafe fn device(ev: *mut ffi::libinput_event) -> Device {
    // SAFETY: `ev` is valid (caller) and every event has a device; the name
    // strings are owned by the device and copied here.
    unsafe {
        let d = ffi::libinput_event_get_device(ev);
        Device {
            name: cstr_or(ffi::libinput_device_get_name(d), "?"),
            sysname: cstr_or(ffi::libinput_device_get_sysname(d), "?"),
            caps: Capability::ALL
                .iter()
                .filter(|(_, c)| ffi::libinput_device_has_capability(d, *c) != 0)
                .map(|(cap, _)| *cap)
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caps_are_listed_by_name() {
        let d = Device {
            name: "QEMU Virtio Tablet".into(),
            sysname: "event3".into(),
            caps: vec![Capability::Pointer],
        };
        assert_eq!(d.caps_list(), "pointer");
        assert!(d.has(Capability::Pointer) && !d.has(Capability::Keyboard));
        let none = Device { caps: vec![], ..d };
        assert_eq!(none.caps_list(), "none");
    }

    #[test]
    fn capability_values_follow_libinput_h() {
        let values: Vec<c_int> = Capability::ALL.iter().map(|(_, v)| *v).collect();
        assert_eq!(values, [0, 1, 2, 3, 4, 5, 6]);
    }
}
