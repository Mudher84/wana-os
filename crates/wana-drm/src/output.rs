//! Output selection: the card, connector, mode and CRTC to drive.

use crate::{sys, Card, CardInfo, Connection, ConnectorInfo, Mode};
use wana_log::{debug, info, warn, Subsystem};

const DRM: Subsystem = Subsystem::Drm;

/// Selected output: card, connector, mode and CRTC.
#[derive(Debug)]
pub struct Output {
    pub card: Card,
    pub conn: ConnectorInfo,
    pub mode: Mode,
    pub crtc: u32,
}

/// Finds the first card (in card-index order) with a connected display
/// and returns it with the selected connector, mode and CRTC. With
/// `devnode`, only that device is tried. Logs every step as `[DRM]`.
pub fn find(devnode: Option<&str>) -> Result<Output, String> {
    let cards: Vec<CardInfo> = match devnode {
        Some(path) => vec![CardInfo {
            name: path.to_string(),
            index: 0,
            devnode: path.into(),
            driver: None,
            connectors: vec![],
        }],
        None => crate::discover::cards().map_err(|e| format!("cannot list /sys/class/drm: {e}"))?,
    };
    if cards.is_empty() {
        return Err("no DRM devices found (is a KMS driver loaded?)".into());
    }
    for c in &cards {
        info!(
            DRM,
            "found {} ({}) device-driver={} connectors=[{}]",
            c.name,
            c.devnode.display(),
            c.driver.as_deref().unwrap_or("?"),
            c.connectors.join(", ")
        );
    }

    let mut out = None;
    for c in &cards {
        match probe(c) {
            Ok(Some(o)) => {
                out = Some(o);
                break;
            }
            Ok(None) => info!(DRM, "{}: no connected display, trying next card", c.name),
            Err(e) => warn!(DRM, "{}: {e}", c.name),
        }
    }
    let out = out.ok_or("no card has a connected display")?;
    Ok(out)
}

/// Opens a card and looks for a connected connector with a usable mode and CRTC.
fn probe(info: &CardInfo) -> Result<Option<Output>, String> {
    let card =
        Card::open(&info.devnode).map_err(|e| format!("open {}: {e}", info.devnode.display()))?;
    let (driver, version) = card
        .driver()
        .map_err(|e| format!("DRM_IOCTL_VERSION: {e}"))?;
    info!(DRM, "{}: opened, driver {driver} {version}", info.name);
    if card.cap(sys::DRM_CAP_DUMB_BUFFER).unwrap_or(0) == 0 {
        return Err("no dumb buffer support".into());
    }
    let res = card.resources().map_err(|e| format!("GETRESOURCES: {e}"))?;
    info!(
        DRM,
        "{}: {} connector(s), {} encoder(s), {} CRTC(s), max {}x{}",
        info.name,
        res.connectors.len(),
        res.encoders.len(),
        res.crtcs.len(),
        res.max_width,
        res.max_height
    );
    for &id in &res.connectors {
        let conn = card
            .connector(id)
            .map_err(|e| format!("GETCONNECTOR {id}: {e}"))?;
        info!(
            DRM,
            "connector {} (id {id}): {:?}, {} mode(s), {}x{} mm",
            conn.name,
            conn.connection,
            conn.modes.len(),
            conn.mm_width,
            conn.mm_height
        );
        for m in &conn.modes {
            debug!(DRM, "  mode {m}");
        }
        if conn.connection != Connection::Connected {
            continue;
        }
        let Some(mode) = crate::mode::select(&conn.modes) else {
            warn!(DRM, "{}: connected but reports no modes", conn.name);
            continue;
        };
        let Some(crtc) = card
            .crtc_for(&res, &conn)
            .map_err(|e| format!("GETENCODER: {e}"))?
        else {
            warn!(DRM, "{}: no CRTC available", conn.name);
            continue;
        };
        info!(DRM, "selected {} on CRTC {crtc}: {mode}", conn.name);
        return Ok(Some(Output {
            card,
            conn,
            mode,
            crtc,
        }));
    }
    Ok(None)
}
