//! The shell's live list of mapped application toplevels.
//!
//! ext-foreign-toplevel-list-v1 is visible only on the privileged shell
//! connection. The compositor sends one server-created handle for every
//! mapped xdg_toplevel. We stage title/app-id updates until done, and remove
//! a handle only after closed.

use wana_client::client::{Connection, Event, Proxy, Val};
use wana_wayland::protocols::ext_foreign_toplevel_list_v1 as proto;

#[derive(Debug, Clone)]
struct Entry {
    handle: Proxy,
    identifier: String,
    title: String,
    app_id: String,
    ready: bool,
}

impl Entry {
    fn label(&self) -> String {
        let raw = if !self.title.trim().is_empty() {
            self.title.trim()
        } else if !self.app_id.trim().is_empty() {
            self.app_id.trim()
        } else {
            self.identifier.trim()
        };
        shorten(raw, 18)
    }
}

#[derive(Debug)]
pub struct Toplevels {
    list: Proxy,
    entries: Vec<Entry>,
}

impl Toplevels {
    pub fn new(list: Proxy) -> Toplevels {
        Toplevels {
            list,
            entries: Vec::new(),
        }
    }

    /// Applies one protocol event. Returns true only when visible dock state
    /// changed and should be redrawn.
    pub fn event(&mut self, conn: &Connection, ev: &Event) -> Result<bool, String> {
        if ev.target == self.list
            && ev.opcode == proto::ext_foreign_toplevel_list_v1::event::TOPLEVEL
        {
            let [Val::NewId(handle)] = &ev.args[..] else {
                return Err("foreign toplevel event without new handle".into());
            };
            self.entries.push(Entry {
                handle: *handle,
                identifier: String::new(),
                title: String::new(),
                app_id: String::new(),
                ready: false,
            });
            return Ok(false);
        }

        let Some(i) = self.entries.iter().position(|t| t.handle == ev.target) else {
            return Ok(false);
        };
        use proto::ext_foreign_toplevel_handle_v1::event::*;
        match (ev.opcode, &ev.args[..]) {
            (IDENTIFIER, [Val::Str(v)]) => self.entries[i].identifier = v.clone(),
            (TITLE, [Val::Str(v)]) => self.entries[i].title = v.clone(),
            (APP_ID, [Val::Str(v)]) => self.entries[i].app_id = v.clone(),
            (DONE, []) => {
                self.entries[i].ready = true;
                return Ok(true);
            }
            (CLOSED, []) => {
                let handle = self.entries.remove(i).handle;
                conn.destroy(
                    handle,
                    proto::ext_foreign_toplevel_handle_v1::request::DESTROY,
                );
                return Ok(true);
            }
            _ => {}
        }
        Ok(false)
    }

    pub fn labels(&self) -> Vec<String> {
        self.entries
            .iter()
            .filter(|e| e.ready)
            .map(Entry::label)
            .collect()
    }

    pub fn len(&self) -> usize {
        self.entries.iter().filter(|e| e.ready).count()
    }
}

fn shorten(s: &str, max: usize) -> String {
    let mut it = s.chars();
    let mut out: String = it.by_ref().take(max).collect();
    if it.next().is_some() {
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::shorten;

    #[test]
    fn labels_shorten_on_character_boundaries() {
        assert_eq!(shorten("Wana OS", 18), "Wana OS");
        assert_eq!(shorten("نافذة عربية طويلة جداً", 8), "نافذة عر…");
    }
}
