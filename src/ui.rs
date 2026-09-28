use crate::api::Item;
use crate::app::{App, MENU, Screen};
use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

const GREEN: Color = Color::Rgb(0x3d, 0xff, 0x5a);
const WHITE: Color = Color::Rgb(0xff, 0xff, 0xff);
const TURQ: Color = Color::Rgb(0x40, 0xe0, 0xd0);
const BLUE: Color = Color::Rgb(0x6f, 0x93, 0xff);
const RED: Color = Color::Rgb(0xff, 0x5c, 0x5c);
const PINK: Color = Color::Rgb(0xff, 0x7a, 0xd0);
const BG: Color = Color::Rgb(0x00, 0x00, 0x00);

fn st(c: Color) -> Style {
    Style::default().fg(c).bg(BG)
}

fn put(buf: &mut Buffer, x: u16, y: u16, s: &str, style: Style, max: u16) {
    let area = buf.area;
    if y >= area.y + area.height || x >= area.x + area.width {
        return;
    }
    let room = (area.x + area.width - x).min(max) as usize;
    let t: String = s.chars().take(room).collect();
    buf.set_string(x, y, t, style);
}

fn fit(s: &str, w: usize) -> String {
    let n = s.chars().count();
    if n <= w {
        format!("{s:<w$}")
    } else if w > 1 {
        let mut t: String = s.chars().take(w - 1).collect();
        t.push('+');
        t
    } else {
        s.chars().take(w).collect()
    }
}

pub fn fmt_time(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, (secs / 60) % 60, secs % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}

fn kind_label(k: &str) -> &'static str {
    match k {
        "MusicArtist" => "ARTIST",
        "MusicAlbum" => "ALBUM",
        "Audio" => "TRACK",
        "Movie" => "MOVIE",
        "Series" => "SERIES",
        "Season" => "SEASON",
        "Episode" => "EPISODE",
        "Playlist" => "PLAYLIST",
        "CollectionFolder" | "UserView" => "LIBRARY",
        "Folder" => "FOLDER",
        "BoxSet" => "COLLECT",
        "MusicVideo" => "VIDEO",
        k if k.starts_with("X:") => "MENU",
        _ => "OBJECT",
    }
}

fn detail(i: &Item) -> String {
    match i.kind.as_str() {
        "Audio" => {
            let a = i.artist();
            match &i.album {
                Some(al) if !al.is_empty() => format!("{a} / {al}"),
                _ => a,
            }
        }
        "MusicAlbum" => {
            let a = i.artist();
            match i.production_year {
                Some(y) => format!("{a} ({y})"),
                None => a,
            }
        }
        "Episode" => {
            let se = match (i.parent_index_number, i.index_number) {
                (Some(s), Some(e)) => format!(" S{s:02}E{e:02}"),
                _ => String::new(),
            };
            format!("{}{se}", i.series_name.clone().unwrap_or_default())
        }
        "Movie" | "Series" => i.production_year.map(|y| y.to_string()).unwrap_or_default(),
        "Season" | "Playlist" | "BoxSet" | "MusicArtist" => {
            i.child_count.map(|c| format!("{c} item(s)")).unwrap_or_default()
        }
        _ => String::new(),
    }
}

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    let buf = f.buffer_mut();
    for y in 0..area.height {
        put(buf, 0, y, &" ".repeat(area.width as usize), st(GREEN), area.width);
    }
    if area.width < 60 || area.height < 16 {
        put(buf, 0, 0, "TERMINAL TOO SMALL - NEED 60x16", st(RED), area.width);
        return;
    }
    let w = area.width;
    let h = area.height;

    // Bottom rows, top to bottom: now-playing strip, message, command label, command, F-keys.
    let np_row = h.saturating_sub(5);
    let msg_row = h.saturating_sub(4);
    let cmd_row = h.saturating_sub(2);
    let key_row = h.saturating_sub(1);

    // Header: title left, system right; user/date underneath.
    let (title, keys): (String, &str) = match app.screen {
        Screen::SignOn => ("Sign On".into(), "F3=Exit"),
        Screen::Menu => (
            "JELLYFIN   Main Menu".into(),
            "F1=Help  F3=Exit  F4=Prompt  F11=Now playing  F12=Cancel",
        ),
        Screen::List => (
            app.current_view().map(|v| v.title.clone()).unwrap_or_default(),
            "F3=Exit F4=Search F5=Refresh F6=Pause F7=Prev F8=Next F9/F10=Vol F11=Now F12=Back",
        ),
        Screen::Queue => (
            "Work with Queue".into(),
            "F3=Exit  F5=Refresh  F6=Pause  F7=Prev  F8=Next  F9/F10=Vol  F12=Cancel",
        ),
        Screen::Now => ("Display Now Playing".into(), "F3=Exit  F6=Pause  F7=Prev  F8=Next  F12=Cancel"),
        Screen::Help => ("Help".into(), "F3=Exit  F12=Cancel"),
    };
    let sys = host(&app.client.cfg.server);
    put(buf, 1, 0, &title, st(WHITE), w);
    let sys_x = w.saturating_sub(sys.chars().count() as u16 + 2);
    put(buf, sys_x, 0, &sys, st(WHITE), w);
    // Tests use a fixed clock so generated screenshots are reproducible.
    #[cfg(test)]
    let when = "28/09/26  20:15:42".to_string();
    #[cfg(not(test))]
    let when = chrono::Local::now().format("%d/%m/%y  %H:%M:%S").to_string();
    let stamp = format!("{}  {when}", app.client.cfg.user.to_uppercase());
    let stamp_x = w.saturating_sub(stamp.chars().count() as u16 + 2);
    put(buf, stamp_x, 1, &stamp, st(GREEN), w);

    // Body
    let body = Rect { x: 0, y: 3, width: w, height: np_row.saturating_sub(3) };
    match app.screen {
        Screen::SignOn => draw_signon(buf, app, body),
        Screen::Menu => draw_menu(buf, body),
        Screen::List | Screen::Queue => draw_list(buf, app, body),
        Screen::Now => draw_now(buf, app, body),
        Screen::Help => draw_help(buf, body),
    }

    // Now-playing strip
    if app.screen != Screen::SignOn && app.screen != Screen::Now {
        if let Some((item, ps)) = app.now_playing() {
            let mark = if ps.paused { "PAUSED " } else { "PLAYING" };
            let text = format!(
                " {mark}  {}{}  {} / {}  VOL {}%",
                item.name,
                if item.artist().is_empty() { String::new() } else { format!(" - {}", item.artist()) },
                fmt_time(ps.pos as u64),
                fmt_time(ps.dur as u64),
                ps.volume as u32
            );
            put(buf, 0, np_row, &fit(&text, w as usize), st(BG).bg(TURQ), w);
        }
    }

    // Message line
    if app.busy && app.message.is_none() {
        put(buf, 1, msg_row, "PLEASE WAIT...", st(WHITE), w);
    } else if let Some(m) = &app.message {
        let s = if m.error { st(RED).add_modifier(Modifier::REVERSED) } else { st(WHITE) };
        put(buf, 1, msg_row, &format!(" {} ", m.text), s, w);
    }

    // Command line
    if matches!(app.screen, Screen::Menu | Screen::List | Screen::Queue) {
        let label = if app.screen == Screen::Menu { "Selection or command" } else { "Command" };
        put(buf, 1, cmd_row.saturating_sub(1), label, st(GREEN), w);
        put(buf, 1, cmd_row, "===>", st(GREEN), w);
        let width = w.saturating_sub(8) as usize;
        let mut txt = app.cmd.clone();
        while txt.chars().count() > width {
            txt.remove(0);
        }
        put(buf, 6, cmd_row, &format!("{txt:<width$}"), st(WHITE).add_modifier(Modifier::UNDERLINED), w);
        let cx = 6 + txt.chars().count() as u16;
        f.set_cursor_position((cx.min(w - 1), cmd_row));
    }
    let buf = f.buffer_mut();
    put(buf, 1, key_row, keys, st(BLUE), w);
}

fn host(server: &str) -> String {
    let s = server.trim_start_matches("https://").trim_start_matches("http://");
    let s = s.split('/').next().unwrap_or("").split(':').next().unwrap_or("");
    let s = s.split('.').next().unwrap_or("");
    if s.is_empty() { "JELLYFIN".into() } else { s.to_uppercase() }
}

fn draw_signon(buf: &mut Buffer, app: &App, body: Rect) {
    let x = 4;
    let mut y = body.y + 1;
    put(buf, x, y, "Type choices, press Enter.", st(GREEN), body.width);
    y += 2;
    let labels = [
        "System  . . . . . . . . . . . :",
        "User  . . . . . . . . . . . . :",
        "Password  . . . . . . . . . . :",
    ];
    for (i, l) in labels.iter().enumerate() {
        put(buf, x, y, l, st(GREEN), body.width);
        let val = if i == 2 { "*".repeat(app.signon.fields[i].chars().count()) } else { app.signon.fields[i].clone() };
        let fw = 40usize;
        let shown: String = {
            let n = val.chars().count();
            if n > fw { val.chars().skip(n - fw).collect() } else { val }
        };
        let style = if app.signon.focus == i {
            st(WHITE).add_modifier(Modifier::UNDERLINED | Modifier::BOLD)
        } else {
            st(PINK).add_modifier(Modifier::UNDERLINED)
        };
        put(buf, x + 33, y, &format!("{shown:<fw$}"), style, body.width);
        y += 2;
    }
    put(buf, x, y, "Trust unverified certificate  :", st(GREEN), body.width);
    let ins_style = if app.signon.focus == 3 {
        st(WHITE).add_modifier(Modifier::UNDERLINED | Modifier::BOLD)
    } else {
        st(PINK).add_modifier(Modifier::UNDERLINED)
    };
    let ins = if app.signon.insecure { "Y" } else { "N" };
    put(buf, x + 33, y, &format!("{ins:<3}"), ins_style, body.width);
    put(buf, x + 38, y, "Y=yes  N=no  (self-signed HTTPS)", st(TURQ), body.width);
    y += 2;
    put(buf, x, y + 1, "System example: http://myserver:8096", st(TURQ), body.width);
}

fn draw_menu(buf: &mut Buffer, body: Rect) {
    put(buf, 1, body.y, "Select one of the following:", st(GREEN), body.width);
    let mut y = body.y + 2;
    for (n, label) in MENU {
        if !n.is_empty() {
            put(buf, 4, y, &format!("{n:>3}.  {label}"), st(GREEN), body.width);
        }
        y += 1;
    }
    y += 1;
    put(
        buf,
        1,
        y,
        "Commands: PLAY  PAUSE  NEXT  PREV  STOP  VOL n  SEARCH text  QUEUE  NOW  EXIT",
        st(TURQ),
        body.width,
    );
}

fn draw_list(buf: &mut Buffer, app: &mut App, body: Rect) {
    let queue_mode = app.screen == Screen::Queue;
    let playing_idx = app.player_state().filter(|s| !s.idle).map(|s| s.index);

    put(buf, 1, body.y, "Type options, press Enter.", st(GREEN), body.width);
    let opt_help = if queue_mode { "  1=Play  4=Remove" } else { "  1=Play  2=Queue  5=Open" };
    put(buf, 1, body.y + 1, opt_help, st(GREEN), body.width);

    let head_y = body.y + 3;
    let first_row = head_y + 1;
    let rows = body.height.saturating_sub(5) as usize;
    app.page = rows.max(1);

    let w = body.width as usize;
    let time_w = 8usize;
    let kind_w = 9usize;
    let det_w = (w / 3).clamp(12, 34);
    let name_w = w.saturating_sub(5 + 1 + kind_w + 1 + det_w + 1 + time_w + 2).max(10);

    let header = format!(
        "Opt  {} {} {} {}",
        fit("Name", name_w - 1),
        fit("Type", kind_w),
        fit("Detail", det_w),
        format!("{:>time_w$}", "Length")
    );
    put(buf, 1, head_y, &header, st(WHITE), body.width);

    let (len, sel, mut top, loading, total) = if queue_mode {
        (app.queue.len(), app.queue_view.sel, app.queue_view.top, false, app.queue.len())
    } else {
        let v = app.stack.last().unwrap();
        (v.items.len(), v.sel, v.top, v.loading, v.total.max(v.items.len()))
    };
    let opts = if queue_mode { &app.queue_view.opts } else { &app.stack.last().unwrap().opts };

    if sel < top {
        top = sel;
    }
    if sel >= top + rows {
        top = sel + 1 - rows;
    }
    let end = len.min(top + rows);
    // Only the visible rows are copied out each frame.
    let visible: Vec<Item> = if queue_mode {
        app.queue[top.min(end)..end].to_vec()
    } else {
        app.stack.last().unwrap().items[top.min(end)..end].to_vec()
    };
    let opts = opts.clone();
    if queue_mode {
        app.queue_view.top = top;
    } else if let Some(v) = app.stack.last_mut() {
        v.top = top;
    }

    if len == 0 {
        let msg = if loading { "Loading..." } else if queue_mode { "Queue is empty." } else { "No records to display." };
        put(buf, 6, first_row + 1, msg, st(TURQ), body.width);
    }

    for (r, it) in visible.iter().enumerate() {
        let idx = top + r;
        let y = first_row + r as u16;
        let selected = idx == sel;
        let opt = opts.get(&idx).copied();
        let opt_txt = match opt {
            Some(c) => format!(" {c} "),
            None => " _ ".into(),
        };
        let opt_style = if opt.is_some() { st(WHITE).add_modifier(Modifier::BOLD) } else { st(GREEN).add_modifier(Modifier::DIM) };
        put(buf, 1, y, &opt_txt, opt_style, body.width);

        let mark = if queue_mode && playing_idx == Some(idx as i64) { ">" } else { " " };
        let name = if it.kind == "Audio" {
            match it.index_number {
                Some(n) if !queue_mode => format!("{n:02}. {}", it.name),
                _ => it.name.clone(),
            }
        } else {
            it.name.clone()
        };
        let line = format!(
            "{mark}{} {} {} {}",
            fit(&name, name_w - 1),
            fit(kind_label(&it.kind), kind_w),
            fit(&detail(it), det_w),
            format!("{:>time_w$}", it.secs().map(fmt_time).unwrap_or_default())
        );
        let style = if selected {
            st(BG).bg(GREEN)
        } else if mark == ">" {
            st(WHITE).add_modifier(Modifier::BOLD)
        } else {
            st(GREEN)
        };
        put(buf, 5, y, &line, style, body.width);
    }

    let more_y = first_row + rows as u16;
    let more = if top + rows < total { "More..." } else { "Bottom" };
    let count = format!("{}/{}  {more}", sel.saturating_add(1).min(total), total);
    put(buf, body.width.saturating_sub(count.chars().count() as u16 + 2), more_y, &count, st(BLUE), body.width);
}

fn bar(width: usize, frac: f64) -> String {
    let filled = ((width as f64) * frac.clamp(0.0, 1.0)).round() as usize;
    format!("{}{}", "#".repeat(filled), ".".repeat(width - filled))
}

fn draw_now(buf: &mut Buffer, app: &App, body: Rect) {
    let x = 4;
    let mut y = body.y + 1;
    let Some((item, ps)) = app.now_playing() else {
        put(buf, x, y, "Nothing is playing.", st(TURQ), body.width);
        put(buf, x, y + 2, "Use option 1 on a track, album, or playlist to start.", st(GREEN), body.width);
        return;
    };
    let rows = [
        ("Title  . . . . . :", item.name.clone()),
        ("Artist . . . . . :", item.artist()),
        ("Album  . . . . . :", item.album.clone().unwrap_or_default()),
        ("Queue position . :", format!("{} of {}", ps.index + 1, app.queue.len())),
        ("Status . . . . . :", if ps.paused { "PAUSED".into() } else { "PLAYING".to_string() }),
        ("Volume . . . . . :", format!("{}%", ps.volume as u32)),
    ];
    for (l, v) in rows {
        put(buf, x, y, l, st(GREEN), body.width);
        put(buf, x + 19, y, &v, st(WHITE), body.width);
        y += 1;
    }
    y += 1;
    let bw = (body.width as usize).saturating_sub(x as usize * 2 + 22).clamp(10, 60);
    let frac = if ps.dur > 0.0 { ps.pos / ps.dur } else { 0.0 };
    put(
        buf,
        x,
        y,
        &format!("{} [{}] {}", fmt_time(ps.pos as u64), bar(bw, frac), fmt_time(ps.dur as u64)),
        st(TURQ),
        body.width,
    );
    y += 2;
    put(
        buf,
        x,
        y,
        "Space=Pause  Left/Right=Seek 10s  Up/Down=Volume  n=Next  b=Previous  Enter=Return",
        st(GREEN),
        body.width,
    );
    y += 2;
    put(buf, x, y, "Up next:", st(WHITE), body.width);
    for (i, it) in app.queue.iter().skip(ps.index as usize + 1).take(5).enumerate() {
        put(buf, x + 2, y + 1 + i as u16, &format!("{} - {}", it.artist(), it.name), st(GREEN), body.width);
    }
}

fn draw_help(buf: &mut Buffer, body: Rect) {
    let lines = [
        "Screens",
        "  Menu         Type a number or command, press Enter.",
        "  Lists        Up/Down/PgUp/PgDn move. Type 1, 2 or 5 in the Opt column, Enter runs it.",
        "               Enter alone opens the highlighted row (or plays a track).",
        "  Command line Type any letters to fill it. Enter runs it. Esc clears it.",
        "",
        "Options",
        "  1=Play  (tracks: plays on through the list; folders: plays everything inside)",
        "  2=Queue  5=Open  4=Remove (queue only)",
        "",
        "Function keys",
        "  F3 Exit   F4 Search   F5 Refresh   F6 Pause   F7 Previous   F8 Next",
        "  F9 Volume down   F10 Volume up   F11 Now playing   F12 or Esc Back",
        "",
        "Commands",
        "  SEARCH text  QUEUE  NOW  PAUSE  NEXT  PREV  STOP  VOL n  MENU  SIGNOFF  EXIT",
        "",
        "Press any key to return.",
    ];
    for (i, l) in lines.iter().enumerate() {
        let s = if l.starts_with(' ') || l.is_empty() || l.starts_with("Press") { st(GREEN) } else { st(WHITE) };
        put(buf, 2, body.y + i as u16, l, s, body.width);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::Client;
    use crate::config::Config;
    use ratatui::{Terminal, backend::TestBackend};

    fn dump(app: &mut App) -> String {
        let mut t = Terminal::new(TestBackend::new(90, 26)).unwrap();
        t.draw(|f| draw(f, app)).unwrap();
        let b = t.backend().buffer();
        (0..b.area.height)
            .map(|y| (0..b.area.width).map(|x| b[(x, y)].symbol().to_string()).collect::<String>().trim_end().to_string())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn screens() {
        let mut cfg = Config::default();
        cfg.server = "http://media.home:8096".into();
        cfg.user = "gio".into();
        let mut app = App::new(Client::new(cfg));
        println!("{}\n", dump(&mut app));
        app.screen = Screen::Menu;
        println!("{}\n", dump(&mut app));
        app.screen = Screen::Queue;
        app.queue = (1..=4)
            .map(|i| Item { id: i.to_string(), name: format!("Track number {i}"), kind: "Audio".into(), artists: vec!["Some Band".into()], album: Some("Great Album".into()), run_time_ticks: Some(2_450_000_000), ..Default::default() })
            .collect();
        app.queue_view.opts.insert(1, '4');
        println!("{}", dump(&mut app));
    }
}

#[cfg(test)]
mod screenshots {
    use super::*;
    use crate::api::{Client, Query};
    use crate::app::ListView;
    use crate::config::Config;
    use crate::player::PlayerState;
    use ratatui::{Terminal, backend::TestBackend};
    use std::collections::HashMap;
    use std::fmt::Write;

    const CW: f32 = 10.0;
    const CH: f32 = 21.0;

    fn hex(c: Color, default: &str) -> String {
        match c {
            Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
            _ => default.to_string(),
        }
    }

    fn svg(app: &mut App, w: u16, h: u16) -> String {
        let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
        t.draw(|f| draw(f, app)).unwrap();
        let b = t.backend().buffer();
        let (pw, ph) = (w as f32 * CW + 24.0, h as f32 * CH + 24.0);
        let mut o = String::new();
        let _ = write!(o, "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{pw}\" height=\"{ph}\" viewBox=\"0 0 {pw} {ph}\">");
        let _ = write!(o, "<rect width=\"100%\" height=\"100%\" rx=\"8\" fill=\"#000\"/>");
        let _ = write!(o, "<g font-family=\"Liberation Mono, DejaVu Sans Mono, monospace\" font-size=\"16.6\" xml:space=\"preserve\">");
        for y in 0..h {
            // Group horizontal runs that share a style.
            let mut x = 0;
            while x < w {
                let c0 = &b[(x, y)];
                let key = (c0.fg, c0.bg, c0.modifier);
                let mut end = x + 1;
                while end < w {
                    let c = &b[(end, y)];
                    if (c.fg, c.bg, c.modifier) != key {
                        break;
                    }
                    end += 1;
                }
                let text: String = (x..end).map(|i| b[(i, y)].symbol().to_string()).collect();
                let (mut fg, mut bg) = (hex(c0.fg, "#3dff5a"), hex(c0.bg, "#000000"));
                if c0.modifier.contains(Modifier::REVERSED) {
                    std::mem::swap(&mut fg, &mut bg);
                }
                let px = 12.0 + x as f32 * CW;
                let py = 12.0 + y as f32 * CH;
                let len = (end - x) as f32 * CW;
                if bg != "#000000" {
                    let _ = write!(o, "<rect x=\"{px}\" y=\"{py}\" width=\"{len}\" height=\"{CH}\" fill=\"{bg}\"/>");
                }
                if !text.trim().is_empty() || c0.modifier.contains(Modifier::UNDERLINED) {
                    let esc = text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
                    let mut extra = String::new();
                    if c0.modifier.contains(Modifier::BOLD) {
                        extra.push_str(" font-weight=\"bold\"");
                    }
                    if c0.modifier.contains(Modifier::UNDERLINED) {
                        extra.push_str(" text-decoration=\"underline\"");
                    }
                    if c0.modifier.contains(Modifier::DIM) {
                        extra.push_str(" opacity=\"0.55\"");
                    }
                    let _ = write!(
                        o,
                        "<text x=\"{px}\" y=\"{}\" fill=\"{fg}\" textLength=\"{len}\" lengthAdjust=\"spacingAndGlyphs\"{extra}>{esc}</text>",
                        py + CH - 5.0
                    );
                }
                x = end;
            }
        }
        o.push_str("</g></svg>");
        o
    }

    fn item(kind: &str, name: &str) -> Item {
        Item { id: name.into(), name: name.into(), kind: kind.into(), ..Default::default() }
    }

    fn track(n: u32, name: &str, secs: u64) -> Item {
        Item {
            index_number: Some(n),
            artists: vec!["Massive Attack".into()],
            album: Some("Mezzanine".into()),
            run_time_ticks: Some(secs * 10_000_000),
            ..item("Audio", name)
        }
    }

    fn view(title: &str, items: Vec<Item>, sel: usize) -> ListView {
        let total = items.len();
        ListView {
            id: 1,
            title: title.into(),
            query: Some(Query::Views),
            items,
            sel,
            top: 0,
            opts: HashMap::new(),
            loading: false,
            more_loading: false,
            total,
        }
    }

    fn app() -> App {
        let cfg = Config {
            server: "https://media.example.org".into(),
            user: "gio".into(),
            token: "x".into(),
            ..Default::default()
        };
        let mut a = App::new(Client::new(cfg));
        a.screen = Screen::Menu;
        a
    }

    fn playing(a: &mut App, index: i64, pos: f64, dur: f64, paused: bool) {
        a.mock_state = Some(PlayerState { pos, dur, paused, index, volume: 80.0, count: 5, idle: false });
    }

    #[test]
    #[ignore]
    fn generate() {
        let tracks = vec![
            track(1, "Angel", 379),
            track(2, "Risingson", 298),
            track(3, "Teardrop", 330),
            track(4, "Inertia Creeps", 356),
            track(5, "Exchange", 251),
            track(6, "Dissolved Girl", 366),
            track(7, "Man Next Door", 355),
            track(8, "Black Milk", 380),
            track(9, "Mezzanine", 356),
            track(10, "Group Four", 491),
            track(11, "(Exchange)", 252),
        ];
        let (w, h) = (100, 28);
        std::fs::create_dir_all("docs").unwrap();
        let save = |name: &str, a: &mut App| std::fs::write(format!("docs/{name}.svg"), svg(a, w, h)).unwrap();

        let mut a = app();
        a.screen = Screen::SignOn;
        a.signon.fields = ["https://media.example.org".into(), "gio".into(), "secret".into()];
        a.signon.focus = 2;
        a.client.cfg.token.clear();
        save("signon", &mut a);

        let mut a = app();
        a.cmd = "".into();
        save("menu", &mut a);

        let mut a = app();
        let albums: Vec<Item> = [
            ("Blue Lines", 1991), ("Protection", 1994), ("Mezzanine", 1998),
            ("100th Window", 2003), ("Heligoland", 2010), ("Ritual Spirit", 2016),
        ]
        .iter()
        .map(|(n, y)| Item {
            artists: vec!["Massive Attack".into()],
            production_year: Some(*y),
            ..item("MusicAlbum", n)
        })
        .collect();
        a.stack.push(view("Albums: Massive Attack", albums, 2));
        a.stack[0].opts.insert(2, '1');
        a.stack[0].opts.insert(4, '2');
        a.screen = Screen::List;
        playing(&mut a, 2, 101.0, 330.0, false);
        a.queue = tracks.clone();
        save("albums", &mut a);

        let mut a = app();
        a.stack.push(view("Mezzanine", tracks.clone(), 3));
        a.screen = Screen::List;
        save("tracks", &mut a);

        let mut a = app();
        let vids = vec![
            Item { series_name: Some("Severance".into()), parent_index_number: Some(2), index_number: Some(4), run_time_ticks: Some(3_120_000_000_0), ..item("Episode", "Woe's Hollow") },
            Item { production_year: Some(1982), run_time_ticks: Some(7_020_000_000_0), ..item("Movie", "Blade Runner") },
            Item { series_name: Some("The Bear".into()), parent_index_number: Some(3), index_number: Some(1), run_time_ticks: Some(2_040_000_000_0), ..item("Episode", "Tomorrow") },
            Item { production_year: Some(1979), run_time_ticks: Some(6_960_000_000_0), ..item("Movie", "Alien") },
        ];
        a.stack.push(view("Continue Watching", vids, 0));
        a.screen = Screen::List;
        save("continue", &mut a);

        let mut a = app();
        a.queue = tracks[..6].to_vec();
        a.screen = Screen::Queue;
        a.queue_view.sel = 3;
        a.queue_view.opts.insert(4, '4');
        playing(&mut a, 2, 101.0, 330.0, false);
        save("queue", &mut a);

        let mut a = app();
        a.queue = tracks.clone();
        a.screen = Screen::Now;
        playing(&mut a, 2, 101.0, 330.0, false);
        save("now-playing", &mut a);
    }
}
