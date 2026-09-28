use crate::config::Config;
use serde::Deserialize;
use std::time::Duration;

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "PascalCase")]
pub struct Item {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(rename = "Type", default)]
    pub kind: String,
    #[serde(default)]
    pub collection_type: Option<String>,
    #[serde(default)]
    pub run_time_ticks: Option<u64>,
    #[serde(default)]
    pub artists: Vec<String>,
    #[serde(default)]
    pub album_artist: Option<String>,
    #[serde(default)]
    pub album: Option<String>,
    #[serde(default)]
    pub index_number: Option<u32>,
    #[serde(default)]
    pub production_year: Option<u32>,
    #[serde(default)]
    pub child_count: Option<u32>,
    #[serde(default)]
    pub user_data: Option<UserData>,
    #[serde(default)]
    pub series_name: Option<String>,
    #[serde(default)]
    pub parent_index_number: Option<u32>,
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "PascalCase")]
pub struct UserData {
    #[serde(default)]
    pub playback_position_ticks: Option<u64>,
}

#[derive(Clone, Copy, PartialEq)]
pub enum ReportKind {
    Start,
    Progress,
    Stop,
}

pub struct Report {
    pub kind: ReportKind,
    pub item_id: String,
    pub ticks: u64,
    pub paused: bool,
}

impl Item {
    pub fn is_playable(&self) -> bool {
        matches!(self.kind.as_str(), "Audio" | "Movie" | "Episode" | "Video" | "MusicVideo")
    }

    pub fn is_video(&self) -> bool {
        matches!(self.kind.as_str(), "Movie" | "Episode" | "Video" | "MusicVideo")
    }

    pub fn secs(&self) -> Option<u64> {
        self.run_time_ticks.map(|t| t / 10_000_000)
    }

    /// Saved position to resume from. Video only, and not when nearly finished.
    pub fn resume_secs(&self) -> f64 {
        if !self.is_video() {
            return 0.0;
        }
        let pos = self.user_data.as_ref().and_then(|u| u.playback_position_ticks).unwrap_or(0);
        match self.run_time_ticks {
            Some(total) if pos > 0 && pos < total / 100 * 95 => (pos / 10_000_000) as f64,
            _ => 0.0,
        }
    }

    pub fn artist(&self) -> String {
        self.artists
            .first()
            .cloned()
            .or_else(|| self.album_artist.clone())
            .unwrap_or_default()
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct AuthResult {
    access_token: String,
    user: AuthUser,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct AuthUser {
    id: String,
}

/// What a list screen shows. Cloneable so we can navigate back.
#[derive(Clone, Debug)]
pub enum Query {
    Views,
    Children(String),
    Recursive { parent: String, types: &'static str },
    AlbumArtists(String),
    ArtistAlbums(String),
    Playlists,
    Search(String),
    Resume,
    NextUp,
    Latest,
}

/// Items per page for list queries.
pub const PAGE: usize = 200;

#[derive(Clone)]
pub struct Client {
    http: reqwest::blocking::Client,
    pub cfg: Config,
}

fn build_http(insecure: bool) -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .danger_accept_invalid_certs(insecure)
        .build()
        .expect("http client")
}

const FIELDS: &str = "ChildCount,ProductionYear";

impl Client {
    pub fn new(cfg: Config) -> Client {
        let http = build_http(cfg.insecure);
        Client { http, cfg }
    }

    pub fn set_insecure(&mut self, insecure: bool) {
        self.cfg.insecure = insecure;
        self.http = build_http(insecure);
    }

    fn auth_header(&self) -> String {
        let mut h = format!(
            "MediaBrowser Client=\"jf400\", Device=\"terminal\", DeviceId=\"{}\", Version=\"{}\"",
            self.cfg.device_id,
            env!("CARGO_PKG_VERSION")
        );
        if !self.cfg.token.is_empty() {
            h.push_str(&format!(", Token=\"{}\"", self.cfg.token));
        }
        h
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.cfg.server.trim_end_matches('/'), path)
    }

    /// Returns the items plus the server-side total (for paging).
    fn get_items(&self, path: &str, params: &[(&str, String)]) -> Result<(Vec<Item>, usize), String> {
        let resp = self
            .http
            .get(self.url(path))
            .header("Authorization", self.auth_header())
            .query(params)
            .send()
            .map_err(|e| e.to_string())?;
        if resp.status().as_u16() == 401 {
            return Err("SESSION EXPIRED - SIGN ON AGAIN".into());
        }
        if !resp.status().is_success() {
            return Err(format!("SERVER RETURNED HTTP {}", resp.status().as_u16()));
        }
        let v: serde_json::Value = resp.json().map_err(|e| e.to_string())?;
        // Most endpoints wrap results in {Items, TotalRecordCount}; a few return a bare array.
        let (raw, total) = match v {
            serde_json::Value::Array(_) => (v, None),
            mut o => {
                let total = o["TotalRecordCount"].as_u64().map(|n| n as usize);
                (o["Items"].take(), total)
            }
        };
        let items: Vec<Item> = serde_json::from_value(raw).map_err(|e| e.to_string())?;
        let total = total.unwrap_or(items.len());
        Ok((items, total))
    }

    pub fn sign_on(&mut self, server: &str, user: &str, pass: &str) -> Result<(), String> {
        self.cfg.server = server.trim().trim_end_matches('/').to_string();
        if !self.cfg.server.starts_with("http") {
            self.cfg.server = format!("http://{}", self.cfg.server);
        }
        self.cfg.token.clear();
        let body = serde_json::json!({ "Username": user, "Pw": pass });
        let resp = self
            .http
            .post(self.url("/Users/AuthenticateByName"))
            .header("Authorization", self.auth_header())
            .json(&body)
            .send()
            .map_err(|e| format!("CANNOT REACH SYSTEM: {e}"))?;
        if resp.status().as_u16() == 401 {
            return Err("USER ID OR PASSWORD NOT CORRECT".into());
        }
        if !resp.status().is_success() {
            return Err(format!("SIGN ON FAILED - HTTP {}", resp.status().as_u16()));
        }
        let auth: AuthResult = resp.json().map_err(|e| e.to_string())?;
        self.cfg.user = user.to_string();
        self.cfg.token = auth.access_token;
        self.cfg.user_id = auth.user.id;
        self.cfg.save();
        Ok(())
    }

    pub fn fetch(&self, q: &Query, start: usize) -> Result<(Vec<Item>, usize), String> {
        let uid = self.cfg.user_id.clone();
        let page = |mut p: Vec<(&'static str, String)>| {
            p.push(("StartIndex", start.to_string()));
            p.push(("Limit", PAGE.to_string()));
            p
        };
        match q {
            Query::Views => self.get_items("/UserViews", &[("userId", uid)]),
            Query::Children(parent) => self.get_items(
                "/Items",
                &page(vec![
                    ("userId", uid),
                    ("ParentId", parent.clone()),
                    ("SortBy", "ParentIndexNumber,IndexNumber,SortName".into()),
                    ("Fields", FIELDS.into()),
                ]),
            ),
            Query::Recursive { parent, types } => self.get_items(
                "/Items",
                &page(vec![
                    ("userId", uid),
                    ("ParentId", parent.clone()),
                    ("Recursive", "true".into()),
                    ("IncludeItemTypes", (*types).into()),
                    ("SortBy", "SortName".into()),
                    ("Fields", FIELDS.into()),
                ]),
            ),
            Query::AlbumArtists(parent) => self.get_items(
                "/Artists/AlbumArtists",
                &page(vec![
                    ("userId", uid),
                    ("ParentId", parent.clone()),
                    ("SortBy", "SortName".into()),
                ]),
            ),
            Query::ArtistAlbums(artist) => self.get_items(
                "/Items",
                &page(vec![
                    ("userId", uid),
                    ("ArtistIds", artist.clone()),
                    ("Recursive", "true".into()),
                    ("IncludeItemTypes", "MusicAlbum".into()),
                    ("SortBy", "ProductionYear,SortName".into()),
                    ("Fields", FIELDS.into()),
                ]),
            ),
            Query::Playlists => self.get_items(
                "/Items",
                &page(vec![
                    ("userId", uid),
                    ("Recursive", "true".into()),
                    ("IncludeItemTypes", "Playlist".into()),
                    ("SortBy", "SortName".into()),
                    ("Fields", FIELDS.into()),
                ]),
            ),
            Query::Search(term) => self.get_items(
                "/Items",
                &page(vec![
                    ("userId", uid),
                    ("searchTerm", term.clone()),
                    ("Recursive", "true".into()),
                    (
                        "IncludeItemTypes",
                        "Audio,MusicAlbum,MusicArtist,Movie,Series,Episode,Playlist".into(),
                    ),
                    ("Fields", FIELDS.into()),
                ]),
            ),
            Query::Resume => self.get_items(
                &format!("/Users/{uid}/Items/Resume"),
                &[
                    ("Limit", "50".into()),
                    ("MediaTypes", "Video,Audio".into()),
                    ("Fields", FIELDS.into()),
                ],
            ),
            Query::NextUp => self.get_items(
                "/Shows/NextUp",
                &[("userId", uid), ("Limit", "50".into()), ("Fields", FIELDS.into())],
            ),
            Query::Latest => self.get_items(
                "/Items/Latest",
                &[("userId", uid), ("Limit", "50".into()), ("Fields", FIELDS.into())],
            ),
        }
    }

    /// Every playable item under a folder-like item, in play order.
    pub fn playables_under(&self, item: &Item) -> Result<Vec<Item>, String> {
        let uid = self.cfg.user_id.clone();
        let mut params = vec![
            ("userId", uid),
            ("Recursive", "true".to_string()),
            ("IncludeItemTypes", "Audio,Movie,Episode,MusicVideo".to_string()),
        ];
        match item.kind.as_str() {
            "MusicArtist" => {
                params.push(("ArtistIds", item.id.clone()));
                params.push(("SortBy", "Album,ParentIndexNumber,IndexNumber,SortName".into()));
            }
            "Playlist" => {
                // Playlist order must be preserved, so no SortBy.
                params.push(("ParentId", item.id.clone()));
            }
            _ => {
                params.push(("ParentId", item.id.clone()));
                params.push(("SortBy", "ParentIndexNumber,IndexNumber,SortName".into()));
            }
        }
        self.get_items("/Items", &params).map(|(items, _)| items)
    }

    pub fn send_report(&self, r: &Report) -> Result<(), String> {
        let path = match r.kind {
            ReportKind::Start => "/Sessions/Playing",
            ReportKind::Progress => "/Sessions/Playing/Progress",
            ReportKind::Stop => "/Sessions/Playing/Stopped",
        };
        let body = serde_json::json!({
            "ItemId": r.item_id,
            "PositionTicks": r.ticks,
            "IsPaused": r.paused,
            "CanSeek": true,
            "PlayMethod": "DirectPlay",
        });
        let resp = self
            .http
            .post(self.url(path))
            .header("Authorization", self.auth_header())
            .json(&body)
            .send()
            .map_err(|e| e.to_string())?;
        if resp.status().is_success() { Ok(()) } else { Err(format!("HTTP {}", resp.status().as_u16())) }
    }

    pub fn stream_url(&self, item: &Item) -> String {
        let route = if item.is_video() { "Videos" } else { "Audio" };
        self.url(&format!(
            "/{}/{}/stream?static=true&api_key={}",
            route, item.id, self.cfg.token
        ))
    }
}
