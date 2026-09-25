//! Safe wrapper around an open DRM card (primary node).

use crate::connector::{self, Connection};
use crate::mode::Mode;
use crate::sys::{self, *};
use std::fs::{File, OpenOptions};
use std::io::{self, Read};
use std::os::fd::{AsRawFd, RawFd};
use std::path::Path;

/// An open DRM device.
#[derive(Debug)]
pub struct Card {
    file: File,
}

/// Mode-setting resources of a card.
#[derive(Debug, Clone, Default)]
pub struct Resources {
    pub crtcs: Vec<u32>,
    pub connectors: Vec<u32>,
    pub encoders: Vec<u32>,
    pub max_width: u32,
    pub max_height: u32,
}

/// A connector with its current state and modes.
#[derive(Debug, Clone)]
pub struct ConnectorInfo {
    pub id: u32,
    pub name: String,
    pub connection: Connection,
    pub modes: Vec<Mode>,
    pub encoders: Vec<u32>,
    /// Encoder currently driving this connector (0 = none).
    pub encoder_id: u32,
    pub mm_width: u32,
    pub mm_height: u32,
}

/// A CPU-mappable "dumb" buffer, the simplest scanout buffer KMS offers.
#[derive(Debug, Clone, Copy)]
pub struct DumbBuffer {
    pub handle: u32,
    pub width: u32,
    pub height: u32,
    pub pitch: u32,
    pub size: u64,
}

/// A page-flip completion event.
#[derive(Debug, Clone, Copy)]
pub struct FlipEvent {
    pub user_data: u64,
    pub crtc_id: u32,
    pub sequence: u32,
    /// Completion time in microseconds (CLOCK_MONOTONIC when the card
    /// reports DRM_CAP_TIMESTAMP_MONOTONIC).
    pub time_us: u64,
}

fn ptr<T>(v: &mut [T]) -> u64 {
    v.as_mut_ptr() as u64
}

impl Card {
    pub fn open(path: &Path) -> io::Result<Card> {
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        Ok(Card { file })
    }

    fn fd(&self) -> RawFd {
        self.file.as_raw_fd()
    }

    /// Kernel driver name (e.g. `virtio_gpu`) and version.
    pub fn driver(&self) -> io::Result<(String, String)> {
        let mut v = DrmVersion::default();
        // SAFETY: first call with all lengths 0: the kernel only fills lengths.
        unsafe { drm_ioctl(self.fd(), DRM_IOCTL_VERSION, &mut v)? };
        let mut name = vec![0u8; v.name_len];
        let mut desc = vec![0u8; v.desc_len];
        let mut date = vec![0u8; v.date_len];
        v.name = ptr(&mut name);
        v.desc = ptr(&mut desc);
        v.date = ptr(&mut date);
        // SAFETY: each pointer refers to a buffer of exactly the length the
        // kernel reported, alive until the call returns.
        unsafe { drm_ioctl(self.fd(), DRM_IOCTL_VERSION, &mut v)? };
        name.truncate(v.name_len);
        let ver = format!(
            "{}.{}.{}",
            v.version_major, v.version_minor, v.version_patchlevel
        );
        Ok((String::from_utf8_lossy(&name).into_owned(), ver))
    }

    pub fn cap(&self, capability: u64) -> io::Result<u64> {
        let mut c = DrmGetCap {
            capability,
            value: 0,
        };
        // SAFETY: DrmGetCap is the argument of DRM_IOCTL_GET_CAP; no pointers.
        unsafe { drm_ioctl(self.fd(), DRM_IOCTL_GET_CAP, &mut c)? };
        Ok(c.value)
    }

    pub fn resources(&self) -> io::Result<Resources> {
        loop {
            let mut r = DrmModeCardRes::default();
            // SAFETY: counts are 0, so the kernel writes no arrays.
            unsafe { drm_ioctl(self.fd(), DRM_IOCTL_MODE_GETRESOURCES, &mut r)? };
            let (nc, nk, ne) = (r.count_crtcs, r.count_connectors, r.count_encoders);
            let mut crtcs = vec![0u32; nc as usize];
            let mut connectors = vec![0u32; nk as usize];
            let mut encoders = vec![0u32; ne as usize];
            let mut r2 = DrmModeCardRes {
                crtc_id_ptr: ptr(&mut crtcs),
                connector_id_ptr: ptr(&mut connectors),
                encoder_id_ptr: ptr(&mut encoders),
                count_crtcs: nc,
                count_connectors: nk,
                count_encoders: ne,
                ..Default::default()
            };
            // SAFETY: each array holds exactly the count passed alongside it.
            unsafe { drm_ioctl(self.fd(), DRM_IOCTL_MODE_GETRESOURCES, &mut r2)? };
            // A hotplug between the two calls changes the counts: retry.
            if (r2.count_crtcs, r2.count_connectors, r2.count_encoders) != (nc, nk, ne) {
                continue;
            }
            return Ok(Resources {
                crtcs,
                connectors,
                encoders,
                max_width: r2.max_width,
                max_height: r2.max_height,
            });
        }
    }

    pub fn connector(&self, id: u32) -> io::Result<ConnectorInfo> {
        loop {
            let mut c = DrmModeGetConnector {
                connector_id: id,
                ..Default::default()
            };
            // SAFETY: counts are 0, so the kernel writes no arrays (it may
            // probe the connector, which is what we want).
            unsafe { drm_ioctl(self.fd(), DRM_IOCTL_MODE_GETCONNECTOR, &mut c)? };
            let (nm, ne) = (c.count_modes, c.count_encoders);
            let mut modes = vec![DrmModeModeInfo::default(); nm as usize];
            let mut encoders = vec![0u32; ne as usize];
            let mut c2 = DrmModeGetConnector {
                connector_id: id,
                modes_ptr: ptr(&mut modes),
                encoders_ptr: ptr(&mut encoders),
                count_modes: nm,
                count_encoders: ne,
                ..Default::default()
            };
            // SAFETY: arrays sized to the counts passed; props are not requested.
            unsafe { drm_ioctl(self.fd(), DRM_IOCTL_MODE_GETCONNECTOR, &mut c2)? };
            if (c2.count_modes, c2.count_encoders) != (nm, ne) {
                continue;
            }
            return Ok(ConnectorInfo {
                id,
                name: connector::name(c2.connector_type, c2.connector_type_id),
                connection: Connection::from(c2.connection),
                modes: modes.into_iter().map(|raw| Mode { raw }).collect(),
                encoders,
                encoder_id: c2.encoder_id,
                mm_width: c2.mm_width,
                mm_height: c2.mm_height,
            });
        }
    }

    pub fn encoder(&self, id: u32) -> io::Result<DrmModeGetEncoder> {
        let mut e = DrmModeGetEncoder {
            encoder_id: id,
            ..Default::default()
        };
        // SAFETY: no pointers in DrmModeGetEncoder.
        unsafe { drm_ioctl(self.fd(), DRM_IOCTL_MODE_GETENCODER, &mut e)? };
        Ok(e)
    }

    /// Finds a CRTC able to drive `conn`: the one already attached to its
    /// current encoder if any, otherwise the first CRTC one of its encoders
    /// can use (`possible_crtcs` is a bitmask over `res.crtcs`).
    pub fn crtc_for(&self, res: &Resources, conn: &ConnectorInfo) -> io::Result<Option<u32>> {
        if conn.encoder_id != 0 {
            let enc = self.encoder(conn.encoder_id)?;
            if enc.crtc_id != 0 {
                return Ok(Some(enc.crtc_id));
            }
        }
        for &enc_id in &conn.encoders {
            let enc = self.encoder(enc_id)?;
            for (i, &crtc) in res.crtcs.iter().enumerate() {
                if i < 32 && enc.possible_crtcs & (1 << i) != 0 {
                    return Ok(Some(crtc));
                }
            }
        }
        Ok(None)
    }

    pub fn create_dumb(&self, width: u32, height: u32) -> io::Result<DumbBuffer> {
        let mut d = DrmModeCreateDumb {
            width,
            height,
            bpp: 32,
            ..Default::default()
        };
        // SAFETY: no pointers in DrmModeCreateDumb.
        unsafe { drm_ioctl(self.fd(), DRM_IOCTL_MODE_CREATE_DUMB, &mut d)? };
        Ok(DumbBuffer {
            handle: d.handle,
            width,
            height,
            pitch: d.pitch,
            size: d.size,
        })
    }

    pub fn map_dumb(&self, buf: &DumbBuffer) -> io::Result<sys::Mapping> {
        let mut m = DrmModeMapDumb {
            handle: buf.handle,
            ..Default::default()
        };
        // SAFETY: no pointers in DrmModeMapDumb.
        unsafe { drm_ioctl(self.fd(), DRM_IOCTL_MODE_MAP_DUMB, &mut m)? };
        sys::Mapping::new(self.fd(), m.offset, buf.size as usize)
    }

    pub fn destroy_dumb(&self, buf: &DumbBuffer) -> io::Result<()> {
        let mut d = DrmModeDestroyDumb { handle: buf.handle };
        // SAFETY: no pointers.
        unsafe { drm_ioctl(self.fd(), DRM_IOCTL_MODE_DESTROY_DUMB, &mut d) }
    }

    /// Registers `buf` as an XRGB8888 framebuffer (depth 24, 32 bpp).
    pub fn add_fb(&self, buf: &DumbBuffer) -> io::Result<u32> {
        let mut f = DrmModeFbCmd {
            width: buf.width,
            height: buf.height,
            pitch: buf.pitch,
            bpp: 32,
            depth: 24,
            handle: buf.handle,
            ..Default::default()
        };
        // SAFETY: no pointers in DrmModeFbCmd.
        unsafe { drm_ioctl(self.fd(), DRM_IOCTL_MODE_ADDFB, &mut f)? };
        Ok(f.fb_id)
    }

    pub fn rm_fb(&self, fb_id: u32) -> io::Result<()> {
        let mut id = fb_id;
        // SAFETY: the argument of RMFB is a single u32.
        unsafe { drm_ioctl(self.fd(), DRM_IOCTL_MODE_RMFB, &mut id) }
    }

    /// Legacy modeset: show `fb_id` on `crtc_id` driving `connector_id` in `mode`.
    pub fn set_crtc(
        &self,
        crtc_id: u32,
        fb_id: u32,
        connector_id: u32,
        mode: &Mode,
    ) -> io::Result<()> {
        let mut connectors = [connector_id];
        let mut c = DrmModeCrtc {
            set_connectors_ptr: ptr(&mut connectors),
            count_connectors: 1,
            crtc_id,
            fb_id,
            mode_valid: 1,
            mode: mode.raw,
            ..Default::default()
        };
        // SAFETY: set_connectors_ptr points to a 1-element array alive for the call.
        unsafe { drm_ioctl(self.fd(), DRM_IOCTL_MODE_SETCRTC, &mut c) }
    }

    /// Schedules `fb_id` for the next vblank; completion is reported as a
    /// FlipEvent carrying `user_data`.
    pub fn page_flip(&self, crtc_id: u32, fb_id: u32, user_data: u64) -> io::Result<()> {
        let mut p = DrmModeCrtcPageFlip {
            crtc_id,
            fb_id,
            flags: DRM_MODE_PAGE_FLIP_EVENT,
            reserved: 0,
            user_data,
        };
        // SAFETY: no pointers in DrmModeCrtcPageFlip.
        unsafe { drm_ioctl(self.fd(), DRM_IOCTL_MODE_PAGE_FLIP, &mut p) }
    }

    /// Blocks until DRM events arrive; returns the page-flip completions.
    pub fn read_flip_events(&self) -> io::Result<Vec<FlipEvent>> {
        let mut buf = [0u8; 1024];
        let n = (&self.file).read(&mut buf)?;
        Ok(parse_events(&buf[..n]))
    }
}

/// Parses the byte stream read from a DRM fd into flip-complete events.
fn parse_events(mut bytes: &[u8]) -> Vec<FlipEvent> {
    let mut out = Vec::new();
    let header = std::mem::size_of::<DrmEvent>();
    while bytes.len() >= header {
        let typ = u32::from_ne_bytes(bytes[0..4].try_into().unwrap());
        let len = u32::from_ne_bytes(bytes[4..8].try_into().unwrap()) as usize;
        if len < header || len > bytes.len() {
            break;
        }
        if typ == DRM_EVENT_FLIP_COMPLETE && len >= std::mem::size_of::<DrmEventVblank>() {
            let u32_at = |o: usize| u32::from_ne_bytes(bytes[o..o + 4].try_into().unwrap());
            let user_data = u64::from_ne_bytes(bytes[8..16].try_into().unwrap());
            out.push(FlipEvent {
                user_data,
                time_us: u64::from(u32_at(16)) * 1_000_000 + u64::from(u32_at(20)),
                sequence: u32_at(24),
                crtc_id: u32_at(28),
            });
        }
        bytes = &bytes[len..];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(typ: u32, user: u64, sec: u32, usec: u32, seq: u32, crtc: u32) -> Vec<u8> {
        let mut v = Vec::new();
        for x in [typ, 32] {
            v.extend_from_slice(&x.to_ne_bytes());
        }
        v.extend_from_slice(&user.to_ne_bytes());
        for x in [sec, usec, seq, crtc] {
            v.extend_from_slice(&x.to_ne_bytes());
        }
        v
    }

    #[test]
    fn parses_flip_events_and_skips_others() {
        let mut bytes = event(DRM_EVENT_FLIP_COMPLETE, 7, 10, 500_000, 42, 31);
        bytes.extend(event(0x01, 0, 0, 0, 0, 0)); // vblank event, ignored
        bytes.extend(event(DRM_EVENT_FLIP_COMPLETE, 8, 10, 516_667, 43, 31));
        let ev = parse_events(&bytes);
        assert_eq!(ev.len(), 2);
        assert_eq!(
            (ev[0].user_data, ev[0].sequence, ev[0].crtc_id),
            (7, 42, 31)
        );
        assert_eq!(ev[1].time_us - ev[0].time_us, 16_667);
    }

    #[test]
    fn truncated_stream_is_ignored_safely() {
        let bytes = event(DRM_EVENT_FLIP_COMPLETE, 1, 0, 0, 0, 0);
        assert!(parse_events(&bytes[..20]).is_empty());
        assert!(parse_events(&[]).is_empty());
    }
}
