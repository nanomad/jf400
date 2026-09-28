//! MPRIS integration so media keys and desktop widgets control playback.
use crate::api::Item;
use crate::player::PlayerState;
use souvlaki::{MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig};
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

pub struct Media {
    controls: MediaControls,
    pub events: Receiver<MediaControlEvent>,
    shown: Option<(i64, String)>,
    state: Option<(bool, bool)>,
}

impl Media {
    pub fn new() -> Option<Media> {
        let mut controls = MediaControls::new(PlatformConfig {
            dbus_name: "jf400",
            display_name: "jf400",
            hwnd: None,
        })
        .ok()?;
        let (tx, events) = channel();
        controls.attach(move |e| {
            let _ = tx.send(e);
        })
        .ok()?;
        Some(Media { controls, events, shown: None, state: None })
    }

    /// Push the current track and play state to the desktop, only when they change.
    pub fn update(&mut self, now: Option<(&Item, &PlayerState)>) {
        let Some((item, ps)) = now else {
            if self.state.take().is_some() {
                self.shown = None;
                let _ = self.controls.set_playback(MediaPlayback::Stopped);
            }
            return;
        };
        let key = (ps.index, item.id.clone());
        if self.shown.as_ref() != Some(&key) {
            let artist = item.artist();
            let album = item.album.clone().unwrap_or_default();
            let _ = self.controls.set_metadata(MediaMetadata {
                title: Some(&item.name),
                artist: (!artist.is_empty()).then_some(artist.as_str()),
                album: (!album.is_empty()).then_some(album.as_str()),
                cover_url: None,
                duration: item.secs().map(Duration::from_secs),
            });
            self.shown = Some(key);
            self.state = None;
        }
        // Position is only re-sent on state changes; desktops extrapolate from it.
        let st = (ps.paused, true);
        if self.state != Some(st) {
            let progress = Some(MediaPosition(Duration::from_secs_f64(ps.pos.max(0.0))));
            let _ = self.controls.set_playback(if ps.paused {
                MediaPlayback::Paused { progress }
            } else {
                MediaPlayback::Playing { progress }
            });
            self.state = Some(st);
        }
    }
}
