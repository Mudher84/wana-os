//! wl_shm buffers backed by a memfd: XRGB8888 pixels written once, then
//! attached. The compositor copies at commit and releases at once
//! (decision in Phase 10 step 3), so a buffer can be destroyed right after
//! the commit that uses it.

use crate::client::{Connection, Proxy, Req};
use std::fs::File;
use std::io::Write;
use std::os::unix::io::{AsRawFd, FromRawFd};
use wana_wayland::protocols::wayland;

const FORMAT_XRGB8888: u32 = 1;

extern "C" {
    fn memfd_create(name: *const std::os::raw::c_char, flags: u32) -> i32;
}

/// A wl_buffer over its own pool and memfd.
#[derive(Debug)]
pub struct Buffer {
    pub buffer: Proxy,
    pub pool: Proxy,
    pub width: i32,
    pub height: i32,
    file: File,
}

impl Buffer {
    /// Creates a buffer holding `bytes` (XRGB8888, `width` x `height`,
    /// rows packed).
    pub fn new(
        conn: &Connection,
        shm: Proxy,
        width: i32,
        height: i32,
        bytes: &[u8],
    ) -> Result<Buffer, String> {
        if bytes.len() != (width * height * 4) as usize {
            return Err(format!(
                "buffer {width}x{height}: {} bytes of pixels",
                bytes.len()
            ));
        }
        // SAFETY: NUL-terminated name; the fd is owned by the File below.
        let fd = unsafe { memfd_create(c"wana-shm".as_ptr(), 0) };
        if fd < 0 {
            return Err(format!("memfd_create: {}", std::io::Error::last_os_error()));
        }
        // SAFETY: fresh fd from memfd_create.
        let mut file = unsafe { File::from_raw_fd(fd) };
        file.write_all(bytes)
            .map_err(|e| format!("pool write: {e}"))?;
        let pool = conn
            .request(
                shm,
                wayland::wl_shm::request::CREATE_POOL,
                Some((&wayland::WL_SHM_POOL_INTERFACE, 1)),
                &[
                    Req::NewId,
                    Req::Fd(file.as_raw_fd()),
                    Req::Int(bytes.len() as i32),
                ],
            )?
            .ok_or("create_pool returned no object")?;
        let buffer = conn
            .request(
                pool,
                wayland::wl_shm_pool::request::CREATE_BUFFER,
                Some((&wayland::WL_BUFFER_INTERFACE, 1)),
                &[
                    Req::NewId,
                    Req::Int(0),
                    Req::Int(width),
                    Req::Int(height),
                    Req::Int(width * 4),
                    Req::Uint(FORMAT_XRGB8888),
                ],
            )?
            .ok_or("create_buffer returned no object")?;
        Ok(Buffer {
            buffer,
            pool,
            width,
            height,
            file,
        })
    }

    /// The memfd behind the pool (tests shrink it on purpose).
    pub fn file(&self) -> &File {
        &self.file
    }

    /// Attaches to `surface`, damages all of it and commits.
    pub fn commit_to(&self, conn: &Connection, surface: Proxy) -> Result<(), String> {
        conn.request(
            surface,
            wayland::wl_surface::request::ATTACH,
            None,
            &[Req::Object(Some(self.buffer)), Req::Int(0), Req::Int(0)],
        )?;
        conn.request(
            surface,
            wayland::wl_surface::request::DAMAGE,
            None,
            &[
                Req::Int(0),
                Req::Int(0),
                Req::Int(self.width),
                Req::Int(self.height),
            ],
        )?;
        conn.request(surface, wayland::wl_surface::request::COMMIT, None, &[])?;
        Ok(())
    }

    /// Destroys the buffer and its pool (the memfd closes with `self`).
    pub fn destroy(self, conn: &Connection) {
        conn.destroy(self.buffer, wayland::wl_buffer::request::DESTROY);
        conn.destroy(self.pool, wayland::wl_shm_pool::request::DESTROY);
    }
}
