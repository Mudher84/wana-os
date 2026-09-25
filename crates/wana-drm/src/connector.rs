//! Connector types and states.

/// Kernel connector type names, as used in `/sys/class/drm/cardN-<name>`
/// (index = `DRM_MODE_CONNECTOR_*` from `<drm/drm_mode.h>`).
const TYPE_NAMES: [&str; 22] = [
    "Unknown",
    "VGA",
    "DVI-I",
    "DVI-D",
    "DVI-A",
    "Composite",
    "SVIDEO",
    "LVDS",
    "Component",
    "DIN",
    "DP",
    "HDMI-A",
    "HDMI-B",
    "TV",
    "eDP",
    "Virtual",
    "DSI",
    "DPI",
    "Writeback",
    "SPI",
    "USB",
    "Unknown",
];

/// Human-readable connector name, e.g. `HDMI-A-1`.
pub fn name(connector_type: u32, type_id: u32) -> String {
    let t = TYPE_NAMES
        .get(connector_type as usize)
        .copied()
        .unwrap_or("Unknown");
    format!("{t}-{type_id}")
}

/// `drm_mode_get_connector.connection`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Connection {
    Connected,
    Disconnected,
    Unknown,
}

impl From<u32> for Connection {
    fn from(v: u32) -> Self {
        match v {
            1 => Connection::Connected,
            2 => Connection::Disconnected,
            _ => Connection::Unknown,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_match_kernel_sysfs_names() {
        assert_eq!(name(11, 1), "HDMI-A-1");
        assert_eq!(name(10, 2), "DP-2");
        assert_eq!(name(14, 1), "eDP-1");
        assert_eq!(name(15, 1), "Virtual-1");
        assert_eq!(name(999, 1), "Unknown-1");
    }

    #[test]
    fn connection_states() {
        assert_eq!(Connection::from(1), Connection::Connected);
        assert_eq!(Connection::from(2), Connection::Disconnected);
        assert_eq!(Connection::from(3), Connection::Unknown);
    }
}
