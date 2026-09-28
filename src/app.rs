use crate::api::{Client, Item, Query, Report, ReportKind};
use crate::media::Media;
use crate::player::{Player, PlayerState};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender, channel};
use souvlaki::{MediaControlEvent, SeekDirection};
use std::time::{Duration, Instant};

pub enum Msg {
    Loaded { view_id: u64, append: bool, result: Result<(Vec<Item>, usize), String> },
    Playables { enqueue: bool, result: Result<Vec<Item>, String> },
    SignedOn(Result<Client, String>),
}

#[derive(Clone, Copy, PartialEq)]
pub enum Screen {
    SignOn,
    Menu,
    List,
    Queue,
    Now,
    Help,
}

pub struct ListView {
    pub id: u64,
    pub title: String,
    pub query: Option<Query>,
    pub items: Vec<Item>,
    pub sel: usize,
    pub top: usize,
    pub opts: HashMap<usize, char>,
    pub loading: bool,
    pub more_loading: bool,
    pub total: usize,
}

pub struct SignOn {
    pub fields: [String; 3],
    pub focus: usize,
    pub insecure: bool,
}

pub struct Message {
    pub text: String,
    pub error: bool,
}

struct Reported {
    index: i64,
    id: String,
    last_sent: Instant,
    last_pos: f64,
    paused: bool,
}

const PROGRESS_EVERY: Duration = Duration::from_secs(10);

pub struct App {
    #[cfg(test)]
    pub mock_state: Option<PlayerState>,
    media: Option<Media>,
    reporter: Option<Sender<(Client, Report)>>,
    reporter_thread: Option<std::thread::JoinHandle<()>>,
    reported: Option<Reported>,
    pub client: Client,
    pub screen: Screen,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    player: Option<Player>,
    pub queue: Vec<Item>,
    pub stack: Vec<ListView>,
    pub queue_view: ListView,
    pub signon: SignOn,
    pub cmd: String,
    pub message: Option<Message>,
    pub busy: bool,
    pub quit: bool,
    pub page: usize,
    next_view_id: u64,
}

pub const MENU: &[(&str, &str)] = &[
    ("1", "Continue watching"),
    ("2", "Next up"),
    ("3", "Latest additions"),
    ("4", "Work with libraries"),
    ("5", "Work with playlists"),
    ("6", "Search library"),
    ("7", "Work with queue"),
    ("8", "Display now playing"),
    ("", ""),
    ("90", "Sign off"),
];

impl App {
    pub fn new(client: Client) -> App {
        let (tx, rx) = channel();
        let (rtx, rrx) = channel::<(Client, Report)>();
        // One thread keeps start/progress/stop reports in order.
        let reporter_thread = std::thread::spawn(move || {
            for (c, r) in rrx {
                let _ = c.send_report(&r);
            }
        });
        let have_session = !client.cfg.token.is_empty();
        let signon = SignOn {
            fields: [client.cfg.server.clone(), client.cfg.user.clone(), String::new()],
            focus: if client.cfg.server.is_empty() { 0 } else { 2 },
            insecure: client.cfg.insecure,
        };
        App {
            #[cfg(test)]
            mock_state: None,
            media: Media::new(),
            reporter: Some(rtx),
            reporter_thread: Some(reporter_thread),
            reported: None,
            client,
            screen: if have_session { Screen::Menu } else { Screen::SignOn },
            tx,
            rx,
            player: None,
            queue: vec![],
            stack: vec![],
            queue_view: blank_view(0, "Work with Queue"),
            signon,
            cmd: String::new(),
            message: None,
            busy: false,
            quit: false,
            page: 20,
            next_view_id: 1,
        }
    }

    pub fn player_state(&self) -> Option<PlayerState> {
        #[cfg(test)]
        if self.mock_state.is_some() {
            return self.mock_state.clone();
        }
        self.player.as_ref().map(|p| p.snapshot())
    }

    pub fn now_playing(&self) -> Option<(&Item, PlayerState)> {
        let st = self.player_state()?;
        if st.idle || st.index < 0 {
            return None;
        }
        self.queue.get(st.index as usize).map(|i| (i, st))
    }

    pub fn current_view(&self) -> Option<&ListView> {
        self.stack.last()
    }

    fn info(&mut self, t: impl Into<String>) {
        self.message = Some(Message { text: t.into(), error: false });
    }
    fn err(&mut self, t: impl Into<String>) {
        self.message = Some(Message { text: t.into(), error: true });
    }

    // ---- background results ----------------------------------------------

    pub fn drain(&mut self) {
        while let Ok(m) = self.rx.try_recv() {
            match m {
                Msg::Loaded { view_id, append, result } => {
                    let expired = matches!(&result, Err(e) if e.starts_with("SESSION EXPIRED"));
                    if expired {
                        self.session_lost();
                        continue;
                    }
                    let mut msg = None;
                    if let Some(v) = self.stack.iter_mut().find(|v| v.id == view_id) {
                        v.loading = false;
                        v.more_loading = false;
                        match result {
                            Ok((items, total)) => {
                                if items.is_empty() && !append {
                                    msg = Some((false, "NO RECORDS FOUND".to_string()));
                                }
                                v.total = total;
                                if append {
                                    v.items.extend(items);
                                } else {
                                    v.items = items;
                                }
                            }
                            Err(e) => msg = Some((true, e)),
                        }
                    }
                    self.busy = self.stack.iter().any(|v| v.loading);
                    let append_ok = append;
                    match msg {
                        Some((true, t)) => self.err(t),
                        Some((false, t)) => self.info(t),
                        None if !append_ok => self.message = None,
                        None => {}
                    }
                }
                Msg::Playables { enqueue, result } => {
                    self.busy = false;
                    match result {
                        Ok(items) => self.play_items(items, 0, enqueue),
                        Err(e) => self.err(e),
                    }
                }
                Msg::SignedOn(result) => {
                    self.busy = false;
                    match result {
                        Ok(c) => {
                            self.client = c;
                            self.signon.fields[2].clear();
                            self.screen = Screen::Menu;
                            self.message = None;
                        }
                        Err(e) => {
                            self.signon.fields[2].clear();
                            self.signon.focus = 2;
                            self.err(e);
                        }
                    }
                }
            }
        }
    }

    fn report(&self, kind: ReportKind, id: &str, pos: f64, paused: bool) {
        if let Some(tx) = &self.reporter {
            let r = Report {
                kind,
                item_id: id.to_string(),
                ticks: (pos.max(0.0) * 10_000_000.0) as u64,
                paused,
            };
            let _ = tx.send((self.client.clone(), r));
        }
    }

    /// Called every loop: reports start, progress and stop to Jellyfin.
    pub fn tick(&mut self) {
        if self.reap_player() {
            self.info("MPV ENDED - PLAYBACK STOPPED");
        }
        self.load_more();
        self.tick_media();
        if self.client.cfg.token.is_empty() {
            return;
        }
        let now = self.now_playing().map(|(i, s)| (i.id.clone(), s));
        let old = self.reported.take();
        match (old, now) {
            (Some(o), Some((id, s))) if o.index == s.index && o.id == id => {
                let due = o.last_sent.elapsed() >= PROGRESS_EVERY || o.paused != s.paused;
                let last_sent = if due {
                    self.report(ReportKind::Progress, &id, s.pos, s.paused);
                    Instant::now()
                } else {
                    o.last_sent
                };
                self.reported = Some(Reported {
                    index: s.index,
                    id,
                    last_sent,
                    // mpv reports time-pos as null while a file unloads; keep the last real position.
                    last_pos: if s.pos > 0.0 { s.pos } else { o.last_pos },
                    paused: s.paused,
                });
            }
            (old, now) => {
                if let Some(o) = old {
                    self.report(ReportKind::Stop, &o.id, o.last_pos, o.paused);
                }
                if let Some((id, s)) = now {
                    self.report(ReportKind::Start, &id, s.pos, s.paused);
                    self.reported = Some(Reported {
                        index: s.index,
                        id,
                        last_sent: Instant::now(),
                        last_pos: s.pos,
                        paused: s.paused,
                    });
                }
            }
        }
    }

    /// Handle media keys and mirror the current track to the desktop.
    fn tick_media(&mut self) {
        let mut events = vec![];
        if let Some(m) = &self.media {
            while let Ok(e) = m.events.try_recv() {
                events.push(e);
            }
        }
        for e in events {
            match e {
                MediaControlEvent::Play => self.with_player(|p| p.set_pause(false)),
                MediaControlEvent::Pause => self.with_player(|p| p.set_pause(true)),
                MediaControlEvent::Toggle => self.with_player(|p| p.toggle_pause()),
                MediaControlEvent::Next => self.with_player(|p| p.next()),
                MediaControlEvent::Previous => self.with_player(|p| p.prev()),
                MediaControlEvent::Stop => {
                    self.with_player(|p| p.stop());
                    self.queue.clear();
                }
                MediaControlEvent::SeekBy(dir, d) => {
                    let s = d.as_secs() as i64;
                    self.with_player(|p| p.seek(if dir == SeekDirection::Forward { s } else { -s }));
                }
                MediaControlEvent::Seek(dir) => {
                    self.with_player(|p| p.seek(if dir == SeekDirection::Forward { 10 } else { -10 }))
                }
                MediaControlEvent::SetPosition(pos_to) => {
                    self.with_player(|p| p.seek_to(pos_to.0.as_secs_f64()));
                }
                _ => {}
            }
        }
        let st = self.player_state();
        let now = st.as_ref().and_then(|s| {
            if s.idle || s.index < 0 { None } else { self.queue.get(s.index as usize).map(|i| (i, s)) }
        });
        if let Some(m) = self.media.as_mut() {
            m.update(now);
        }
    }

    /// Player is gone: send the Stop for whatever was playing.
    fn tick_stopped(&mut self) {
        if let Some(o) = self.reported.take() {
            self.report(ReportKind::Stop, &o.id, o.last_pos, o.paused);
        }
    }

    /// Flush a final Stop report and wait for the reporter to finish.
    pub fn shutdown(&mut self) {
        if let Some(o) = self.reported.take() {
            self.report(ReportKind::Stop, &o.id, o.last_pos, o.paused);
        }
        self.reporter = None;
        if let Some(t) = self.reporter_thread.take() {
            let _ = t.join();
        }
    }

    fn session_lost(&mut self) {
        self.client.cfg.clear_session();
        self.stack.clear();
        self.busy = false;
        self.screen = Screen::SignOn;
        self.signon.focus = 2;
        self.err("SESSION EXPIRED - SIGN ON AGAIN");
    }

    // ---- navigation --------------------------------------------------------

    fn push_view(&mut self, title: String, query: Query) {
        let id = self.next_view_id;
        self.next_view_id += 1;
        let mut v = blank_view(id, &title);
        v.query = Some(query.clone());
        v.loading = true;
        self.stack.push(v);
        self.screen = Screen::List;
        self.busy = true;
        self.message = None;
        let c = self.client.clone();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Msg::Loaded { view_id: id, append: false, result: c.fetch(&query, 0) });
        });
    }

    fn push_static(&mut self, title: String, items: Vec<Item>) {
        let id = self.next_view_id;
        self.next_view_id += 1;
        let mut v = blank_view(id, &title);
        v.items = items;
        self.stack.push(v);
        self.screen = Screen::List;
    }

    fn refresh(&mut self) {
        let Some(v) = self.stack.last_mut() else { return };
        let Some(q) = v.query.clone() else { return };
        v.loading = true;
        let id = v.id;
        self.busy = true;
        let c = self.client.clone();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Msg::Loaded { view_id: id, append: false, result: c.fetch(&q, 0) });
        });
    }

    /// Fetch the next page when the cursor nears the end of a partly loaded list.
    fn load_more(&mut self) {
        if self.screen != Screen::List {
            return;
        }
        let Some(v) = self.stack.last_mut() else { return };
        let Some(q) = v.query.clone() else { return };
        if v.loading || v.more_loading || v.items.len() >= v.total || v.sel + 50 < v.items.len() {
            return;
        }
        v.more_loading = true;
        let (id, start) = (v.id, v.items.len());
        let c = self.client.clone();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Msg::Loaded { view_id: id, append: true, result: c.fetch(&q, start) });
        });
    }

    pub fn back(&mut self) {
        match self.screen {
            Screen::List => {
                self.stack.pop();
                if self.stack.is_empty() {
                    self.screen = Screen::Menu;
                }
            }
            Screen::Queue | Screen::Now | Screen::Help => {
                self.screen = if self.stack.is_empty() { Screen::Menu } else { Screen::List };
            }
            _ => {}
        }
        self.message = None;
    }

    fn open(&mut self, item: &Item) {
        if let Some(kind) = item.kind.strip_prefix("X:") {
            let (title, q) = match kind {
                "artists" => ("Work with Artists", Query::AlbumArtists(item.id.clone())),
                "albums" => (
                    "Work with Albums",
                    Query::Recursive { parent: item.id.clone(), types: "MusicAlbum" },
                ),
                _ => (
                    "Work with Songs",
                    Query::Recursive { parent: item.id.clone(), types: "Audio" },
                ),
            };
            self.push_view(title.into(), q);
            return;
        }
        if item.is_playable() {
            self.play_from_list(item);
            return;
        }
        match item.kind.as_str() {
            "MusicArtist" => {
                self.push_view(format!("Albums: {}", item.name), Query::ArtistAlbums(item.id.clone()))
            }
            "CollectionFolder" | "UserView"
                if item.collection_type.as_deref() == Some("music") =>
            {
                let cat = |k: &str, n: &str| Item {
                    id: item.id.clone(),
                    name: n.into(),
                    kind: format!("X:{k}"),
                    ..Default::default()
                };
                self.push_static(
                    format!("Music: {}", item.name),
                    vec![cat("artists", "Artists"), cat("albums", "Albums"), cat("songs", "Songs")],
                );
            }
            _ => self.push_view(item.name.clone(), Query::Children(item.id.clone())),
        }
    }

    // ---- playback ----------------------------------------------------------

    /// Drop a player whose mpv process has exited, so the next play starts a fresh one.
    fn reap_player(&mut self) -> bool {
        let dead = self.player.as_mut().is_some_and(|p| !p.alive());
        if dead {
            self.player = None;
            self.queue.clear();
        }
        dead
    }

    fn ensure_player(&mut self) -> bool {
        self.reap_player();
        if self.player.is_none() {
            match Player::spawn(self.client.cfg.insecure) {
                Ok(p) => self.player = Some(p),
                Err(e) => {
                    self.err(e);
                    return false;
                }
            }
        }
        true
    }

    fn play_from_list(&mut self, item: &Item) {
        // Episodes in a season list: play on through the rest of the season.
        if item.kind == "Episode" {
            if let Some(v) = self.stack.last() {
                if matches!(v.query, Some(Query::Children(_))) {
                    let eps: Vec<Item> = v.items.iter().filter(|i| i.kind == "Episode").cloned().collect();
                    if let Some(pos) = eps.iter().position(|i| i.id == item.id) {
                        self.play_items(eps, pos, false);
                        return;
                    }
                }
            }
        }
        // Audio: queue the rest of the visible list so it plays on.
        if !item.is_video() {
            if let Some(v) = self.stack.last() {
                let audio: Vec<Item> = v.items.iter().filter(|i| i.kind == "Audio").cloned().collect();
                if let Some(pos) = audio.iter().position(|i| i.id == item.id) {
                    self.play_items(audio, pos, false);
                    return;
                }
            }
        }
        self.play_items(vec![item.clone()], 0, false);
    }

    fn play_items(&mut self, items: Vec<Item>, start: usize, enqueue: bool) {
        if items.is_empty() {
            self.info("NO PLAYABLE ITEMS FOUND");
            return;
        }
        if !self.ensure_player() {
            return;
        }
        let urls: Vec<String> = items.iter().map(|i| self.client.stream_url(i)).collect();
        let n = items.len();
        let resume: Vec<f64> = items.iter().map(|i| i.resume_secs()).collect();
        let p = self.player.as_mut().unwrap();
        if enqueue {
            p.enqueue(&urls, &resume);
            self.queue.extend(items);
            self.info(format!("{n} ITEM(S) ADDED TO QUEUE"));
        } else {
            p.play_all(&urls, &resume, start);
            self.queue = items;
            self.info(format!("PLAYING - {n} ITEM(S) IN QUEUE"));
        }
    }

    fn play_or_queue(&mut self, item: &Item, enqueue: bool) {
        if item.is_playable() {
            if enqueue {
                self.play_items(vec![item.clone()], 0, true);
            } else {
                self.play_from_list(item);
            }
            return;
        }
        if item.kind.starts_with("X:") {
            self.err("OPTION NOT VALID FOR THIS ENTRY");
            return;
        }
        self.busy = true;
        self.info("LOADING...");
        let c = self.client.clone();
        let tx = self.tx.clone();
        let it = item.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Msg::Playables { enqueue, result: c.playables_under(&it) });
        });
    }

    // ---- commands ----------------------------------------------------------

    fn run_command(&mut self, raw: &str) {
        let raw = raw.trim().to_string();
        let mut parts = raw.splitn(2, ' ');
        let verb = parts.next().unwrap_or("").to_uppercase();
        let arg = parts.next().unwrap_or("").trim().to_string();
        match verb.as_str() {
            "1" | "CONTINUE" => {
                self.stack.clear();
                self.push_view("Continue Watching".into(), Query::Resume);
            }
            "2" | "NEXTUP" => {
                self.stack.clear();
                self.push_view("Next Up".into(), Query::NextUp);
            }
            "3" | "LATEST" | "NEW" => {
                self.stack.clear();
                self.push_view("Latest Additions".into(), Query::Latest);
            }
            "4" | "LIB" | "LIBRARIES" => {
                self.stack.clear();
                self.push_view("Work with Libraries".into(), Query::Views);
            }
            "5" | "PLAYLISTS" => {
                self.stack.clear();
                self.push_view("Work with Playlists".into(), Query::Playlists);
            }
            "6" | "SEARCH" | "FIND" => {
                if arg.is_empty() {
                    self.cmd = "SEARCH ".into();
                    self.info("ENTER SEARCH TERM AFTER THE COMMAND");
                    return;
                }
                self.stack.clear();
                self.push_view(format!("Search: {arg}"), Query::Search(arg));
            }
            "7" | "QUEUE" => self.screen = Screen::Queue,
            "8" | "NOW" | "NP" => self.screen = Screen::Now,
            "90" | "SIGNOFF" => self.sign_off(),
            "MENU" | "MAIN" => {
                self.stack.clear();
                self.screen = Screen::Menu;
            }
            "BACK" => self.back(),
            "EXIT" | "QUIT" | "BYE" => self.quit = true,
            "HELP" => self.screen = Screen::Help,
            "PAUSE" | "RESUME" => self.with_player(|p| p.toggle_pause()),
            "NEXT" => self.with_player(|p| p.next()),
            "PREV" => self.with_player(|p| p.prev()),
            "STOP" => {
                self.with_player(|p| p.stop());
                self.queue.clear();
            }
            "VOL" | "VOLUME" => match arg.parse::<i64>() {
                Ok(n) => {
                    let cur = self.player_state().map(|s| s.volume as i64).unwrap_or(100);
                    self.with_player(|p| p.volume(n.clamp(0, 130) - cur));
                }
                Err(_) => self.err("VOL NEEDS A NUMBER 0-130"),
            },
            "" => {}
            _ => self.err(format!("COMMAND {verb} NOT FOUND")),
        }
    }

    fn with_player(&mut self, f: impl FnOnce(&mut Player)) {
        match self.player.as_mut() {
            Some(p) => f(p),
            None => self.err("NOTHING IS PLAYING"),
        }
    }

    fn sign_off(&mut self) {
        if let Some(mut p) = self.player.take() {
            p.stop();
        }
        self.tick_stopped();
        self.queue.clear();
        self.stack.clear();
        self.client.cfg.clear_session();
        self.signon.fields[2].clear();
        self.signon.focus = 2;
        self.screen = Screen::SignOn;
        self.message = None;
    }

    fn submit_signon(&mut self) {
        let [server, user, pass] = self.signon.fields.clone();
        let insecure = self.signon.insecure;
        if server.trim().is_empty() || user.trim().is_empty() {
            self.err("SYSTEM AND USER ARE REQUIRED");
            return;
        }
        self.busy = true;
        self.info("SIGNING ON...");
        let mut c = self.client.clone();
        c.set_insecure(insecure);
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let r = c.sign_on(&server, &user, &pass).map(|_| c);
            let _ = tx.send(Msg::SignedOn(r));
        });
    }

    // ---- keys --------------------------------------------------------------

    pub fn on_key(&mut self, k: KeyEvent) {
        if k.kind == KeyEventKind::Release {
            return;
        }
        if k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('c') {
            self.quit = true;
            return;
        }
        // Function keys work everywhere.
        if let KeyCode::F(n) = k.code {
            self.on_fkey(n);
            return;
        }
        match self.screen {
            Screen::SignOn => self.key_signon(k),
            Screen::Menu => self.key_menu(k),
            Screen::List | Screen::Queue => self.key_list(k),
            Screen::Now => self.key_now(k),
            Screen::Help => self.back(),
        }
    }

    fn on_fkey(&mut self, n: u8) {
        let signed_on = self.screen != Screen::SignOn;
        match n {
            1 => {
                if signed_on {
                    self.screen = Screen::Help
                }
            }
            3 => self.quit = true,
            4 if signed_on => {
                self.cmd = "SEARCH ".into();
                if matches!(self.screen, Screen::Now | Screen::Help) {
                    self.screen = if self.stack.is_empty() { Screen::Menu } else { Screen::List };
                }
            }
            5 if signed_on => self.refresh(),
            6 if signed_on => self.with_player(|p| p.toggle_pause()),
            7 if signed_on => self.with_player(|p| p.prev()),
            8 if signed_on => self.with_player(|p| p.next()),
            9 if signed_on => self.with_player(|p| p.volume(-5)),
            10 if signed_on => self.with_player(|p| p.volume(5)),
            11 if signed_on => self.screen = Screen::Now,
            12 => self.back(),
            _ => {}
        }
    }

    fn key_signon(&mut self, k: KeyEvent) {
        const N: usize = 4;
        let s = &mut self.signon;
        match k.code {
            KeyCode::Tab | KeyCode::Down => s.focus = (s.focus + 1) % N,
            KeyCode::BackTab | KeyCode::Up => s.focus = (s.focus + N - 1) % N,
            KeyCode::Backspace if s.focus < 3 => {
                s.fields[s.focus].pop();
            }
            KeyCode::Char(c) if s.focus == 3 => match c {
                ' ' => s.insecure = !s.insecure,
                'y' | 'Y' => s.insecure = true,
                'n' | 'N' => s.insecure = false,
                _ => {}
            },
            KeyCode::Char(c) => s.fields[s.focus].push(c),
            KeyCode::Enter => {
                if s.focus < 2 {
                    s.focus += 1;
                } else {
                    self.submit_signon();
                }
            }
            _ => {}
        }
    }

    fn key_menu(&mut self, k: KeyEvent) {
        match k.code {
            KeyCode::Char(c) => self.cmd.push(c),
            KeyCode::Backspace => {
                self.cmd.pop();
            }
            KeyCode::Esc => self.cmd.clear(),
            KeyCode::Enter => {
                let c = std::mem::take(&mut self.cmd);
                self.message = None;
                self.run_command(&c);
            }
            _ => {}
        }
    }

    fn key_now(&mut self, k: KeyEvent) {
        match k.code {
            KeyCode::Char(' ') | KeyCode::Char('p') => self.with_player(|p| p.toggle_pause()),
            KeyCode::Char('n') | KeyCode::Char('>') => self.with_player(|p| p.next()),
            KeyCode::Char('b') | KeyCode::Char('<') => self.with_player(|p| p.prev()),
            KeyCode::Right => self.with_player(|p| p.seek(10)),
            KeyCode::Left => self.with_player(|p| p.seek(-10)),
            KeyCode::Up | KeyCode::Char('+') => self.with_player(|p| p.volume(5)),
            KeyCode::Down | KeyCode::Char('-') => self.with_player(|p| p.volume(-5)),
            KeyCode::Char('q') | KeyCode::Esc | KeyCode::Enter => self.back(),
            _ => {}
        }
    }

    fn view_mut(&mut self) -> &mut ListView {
        if self.screen == Screen::Queue {
            &mut self.queue_view
        } else {
            self.stack.last_mut().expect("list view")
        }
    }

    fn key_list(&mut self, k: KeyEvent) {
        let page = self.page.max(1);
        let queue_mode = self.screen == Screen::Queue;
        let len = if queue_mode { self.queue.len() } else { self.view_mut().items.len() };
        let has_cmd = !self.cmd.is_empty();
        let v = self.view_mut();
        match k.code {
            KeyCode::Up => v.sel = v.sel.saturating_sub(1),
            KeyCode::Down => v.sel = (v.sel + 1).min(len.saturating_sub(1)),
            KeyCode::PageUp => v.sel = v.sel.saturating_sub(page),
            KeyCode::PageDown => v.sel = (v.sel + page).min(len.saturating_sub(1)),
            KeyCode::Home => v.sel = 0,
            KeyCode::End => v.sel = len.saturating_sub(1),
            KeyCode::Esc => {
                // First Esc clears what you typed; Esc on a clean screen goes back.
                if v.opts.is_empty() && !has_cmd {
                    self.back();
                } else {
                    v.opts.clear();
                    self.cmd.clear();
                }
            }
            KeyCode::Backspace => {
                if has_cmd {
                    self.cmd.pop();
                } else {
                    let sel = v.sel;
                    v.opts.remove(&sel);
                }
            }
            KeyCode::Char(c) => {
                if !has_cmd && c.is_ascii_digit() && len > 0 {
                    let sel = v.sel;
                    v.opts.insert(sel, c);
                    v.sel = (sel + 1).min(len - 1);
                } else {
                    self.cmd.push(c);
                }
            }
            KeyCode::Enter => self.enter(),
            _ => {}
        }
    }

    fn enter(&mut self) {
        if !self.cmd.is_empty() {
            let c = std::mem::take(&mut self.cmd);
            self.message = None;
            self.run_command(&c);
            return;
        }
        let queue_mode = self.screen == Screen::Queue;
        let (opts, sel) = {
            let v = self.view_mut();
            let mut o: Vec<(usize, char)> = v.opts.drain().collect();
            o.sort();
            (o, v.sel)
        };
        self.message = None;
        if queue_mode {
            self.queue_options(opts, sel);
            return;
        }
        let items = self.stack.last().map(|v| v.items.clone()).unwrap_or_default();
        if opts.is_empty() {
            if let Some(it) = items.get(sel) {
                let it = it.clone();
                self.open(&it);
            }
            return;
        }
        for (row, o) in opts {
            let Some(it) = items.get(row).cloned() else { continue };
            match o {
                '1' => self.play_or_queue(&it, false),
                '2' => self.play_or_queue(&it, true),
                '5' => self.open(&it),
                _ => self.err(format!("OPTION {o} NOT VALID")),
            }
        }
    }

    fn queue_options(&mut self, opts: Vec<(usize, char)>, sel: usize) {
        if opts.is_empty() {
            if sel < self.queue.len() {
                self.with_player(|p| p.jump(sel));
            }
            return;
        }
        // Remove from the bottom up so indexes stay valid.
        let mut removals = vec![];
        for (row, o) in opts {
            match o {
                '1' => self.with_player(|p| p.jump(row)),
                '4' => removals.push(row),
                _ => self.err(format!("OPTION {o} NOT VALID")),
            }
        }
        removals.sort_unstable_by(|a, b| b.cmp(a));
        for row in removals {
            if row < self.queue.len() {
                self.queue.remove(row);
                self.with_player(|p| p.remove(row));
            }
        }
        let len = self.queue.len();
        self.queue_view.sel = self.queue_view.sel.min(len.saturating_sub(1));
    }
}

fn blank_view(id: u64, title: &str) -> ListView {
    ListView {
        id,
        title: title.to_string(),
        query: None,
        items: vec![],
        sel: 0,
        top: 0,
        opts: HashMap::new(),
        loading: false,
        more_loading: false,
        total: 0,
    }
}
