//! DRM device discovery through sysfs. Never assumes `/dev/dri/card0`:
//! a machine can have several cards (integrated + discrete GPU, or the
//! firmware framebuffer `simpledrm` next to a native driver), and card
//! numbering depends on probe order.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// A DRM primary node found in sysfs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardInfo {
    /// Kernel name, e.g. `card1`.
    pub name: String,
    /// Card index parsed from the name (sort key).
    pub index: u32,
    /// Device node, e.g. `/dev/dri/card1`.
    pub devnode: PathBuf,
    /// Kernel driver bound to the parent device (`virtio_gpu`, `i915`, …).
    pub driver: Option<String>,
    /// Connector names reported by sysfs (`card1-Virtual-1` -> `Virtual-1`).
    pub connectors: Vec<String>,
}

/// Parses `cardN` (primary nodes only; `cardN-HDMI-A-1` is a connector,
/// `renderD128` is a render node).
fn card_index(name: &str) -> Option<u32> {
    let digits = name.strip_prefix("card")?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// Lists DRM cards under `class_dir` (normally `/sys/class/drm`), sorted
/// by card index. `dev_dir` is where device nodes live (`/dev/dri`).
pub fn cards_in(class_dir: &Path, dev_dir: &Path) -> io::Result<Vec<CardInfo>> {
    let mut names = Vec::new();
    for entry in fs::read_dir(class_dir)? {
        names.push(entry?.file_name().to_string_lossy().into_owned());
    }
    let mut cards: Vec<CardInfo> = names
        .iter()
        .filter_map(|name| {
            let index = card_index(name)?;
            let driver = fs::read_link(class_dir.join(name).join("device/driver"))
                .ok()
                .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()));
            let prefix = format!("{name}-");
            let mut connectors: Vec<String> = names
                .iter()
                .filter_map(|n| n.strip_prefix(&prefix).map(str::to_owned))
                .collect();
            connectors.sort();
            Some(CardInfo {
                name: name.clone(),
                index,
                devnode: dev_dir.join(name),
                driver,
                connectors,
            })
        })
        .collect();
    cards.sort_by_key(|c| c.index);
    Ok(cards)
}

/// Lists the cards of the running system.
pub fn cards() -> io::Result<Vec<CardInfo>> {
    cards_in(Path::new("/sys/class/drm"), Path::new("/dev/dri"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn fake_sysfs() -> PathBuf {
        let root = std::env::temp_dir().join(format!("wana-drm-sysfs-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let class = root.join("class/drm");
        let drivers = root.join("bus/drivers");
        for d in ["simple-framebuffer", "virtio_gpu"] {
            fs::create_dir_all(drivers.join(d)).unwrap();
        }
        for (card, drv) in [("card1", "virtio_gpu"), ("card0", "simple-framebuffer")] {
            fs::create_dir_all(class.join(card).join("device")).unwrap();
            symlink(drivers.join(drv), class.join(card).join("device/driver")).unwrap();
        }
        for other in [
            "card1-Virtual-1",
            "card0-Unknown-1",
            "renderD128",
            "version",
            "card1-HDMI-A-1",
        ] {
            fs::create_dir_all(class.join(other)).unwrap();
        }
        root
    }

    #[test]
    fn parses_only_primary_nodes() {
        assert_eq!(card_index("card0"), Some(0));
        assert_eq!(card_index("card12"), Some(12));
        assert_eq!(card_index("card1-HDMI-A-1"), None);
        assert_eq!(card_index("renderD128"), None);
        assert_eq!(card_index("card"), None);
    }

    #[test]
    fn discovers_cards_drivers_and_connectors() {
        let root = fake_sysfs();
        let cards = cards_in(&root.join("class/drm"), Path::new("/dev/dri")).unwrap();
        assert_eq!(cards.len(), 2);
        assert_eq!(cards[0].name, "card0");
        assert_eq!(cards[0].driver.as_deref(), Some("simple-framebuffer"));
        assert_eq!(cards[0].connectors, ["Unknown-1"]);
        assert_eq!(cards[1].devnode, Path::new("/dev/dri/card1"));
        assert_eq!(cards[1].driver.as_deref(), Some("virtio_gpu"));
        assert_eq!(cards[1].connectors, ["HDMI-A-1", "Virtual-1"]);
        fs::remove_dir_all(root).unwrap();
    }
}
