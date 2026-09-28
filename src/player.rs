use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Default)]
pub struct PlayerState {
    pub pos: f64,
    pub dur: f64,
    pub paused: bool,
    pub index: i64,
    pub volume: f64,
    pub count: i64,
    pub idle: bool,
}

pub struct Player {
    child: Child,
    writer: UnixStream,
    pub state: Arc<Mutex<PlayerState>>,
    sock: std::path::PathBuf,
}

impl Player {
    pub fn spawn(insecure: bool) -> Result<Player, String> {
        let dir = std::env::var_os("XDG_RUNTIME_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let sock = dir.join(format!("jf400-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&sock);

        let mut cmd = Command::new("mpv");
        if insecure {
            cmd.arg("--tls-verify=no");
        }
        let child = cmd
            .args([
                "--idle=yes",
                "--no-terminal",
                "--force-window=no",
                "--fullscreen=yes",
                "--audio-display=no",
                "--keep-open=no",
                "--input-default-bindings=yes",
            ])
            .arg(format!("--input-ipc-server={}", sock.display()))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("CANNOT START MPV: {e}"))?;

        let started = Instant::now();
        let stream = loop {
            match UnixStream::connect(&sock) {
                Ok(s) => break s,
                Err(_) if started.elapsed() < Duration::from_secs(4) => {
                    std::thread::sleep(Duration::from_millis(50))
                }
                Err(e) => return Err(format!("MPV IPC NOT AVAILABLE: {e}")),
            }
        };

        let state = Arc::new(Mutex::new(PlayerState {
            volume: 100.0,
            idle: true,
            index: -1,
            ..Default::default()
        }));

        let reader = stream.try_clone().map_err(|e| e.to_string())?;
        let st = state.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(reader).lines().map_while(Result::ok) {
                let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
                if v["event"] != "property-change" {
                    continue;
                }
                let data = &v["data"];
                let mut s = st.lock().unwrap();
                match v["name"].as_str().unwrap_or("") {
                    "time-pos" => s.pos = data.as_f64().unwrap_or(0.0),
                    "duration" => s.dur = data.as_f64().unwrap_or(0.0),
                    "pause" => s.paused = data.as_bool().unwrap_or(false),
                    "playlist-pos" => s.index = data.as_i64().unwrap_or(-1),
                    "playlist-count" => s.count = data.as_i64().unwrap_or(0),
                    "volume" => s.volume = data.as_f64().unwrap_or(100.0),
                    "idle-active" => s.idle = data.as_bool().unwrap_or(true),
                    _ => {}
                }
            }
        });

        let mut p = Player { child, writer: stream, state, sock };
        for (i, name) in [
            "time-pos",
            "duration",
            "pause",
            "playlist-pos",
            "playlist-count",
            "volume",
            "idle-active",
        ]
        .iter()
        .enumerate()
        {
            p.send(json!({ "command": ["observe_property", i + 1, name] }));
        }
        // In the mpv window, q would quit the whole process. Make it just stop playback.
        for key in ["q", "Q"] {
            p.cmd(vec!["keybind".into(), key.into(), "stop".into()]);
        }
        Ok(p)
    }

    fn send(&mut self, v: Value) {
        let mut s = v.to_string();
        s.push('\n');
        let _ = self.writer.write_all(s.as_bytes());
    }

    fn cmd(&mut self, args: Vec<Value>) {
        self.send(json!({ "command": args }));
    }

    fn load(&mut self, url: &str, mode: &str, resume: f64) {
        let mut a: Vec<Value> = vec!["loadfile".into(), url.into(), mode.into()];
        if resume > 0.0 {
            a.push((-1).into());
            a.push(format!("start={resume}").into());
        }
        self.cmd(a);
    }

    /// Replace the playlist and start at `start`. `resume[i]` is a start offset in seconds.
    pub fn play_all(&mut self, urls: &[String], resume: &[f64], start: usize) {
        self.cmd(vec!["playlist-clear".into()]);
        self.cmd(vec!["stop".into()]);
        for (i, u) in urls.iter().enumerate() {
            let mode = if i == 0 { "replace" } else { "append" };
            self.load(u, mode, resume.get(i).copied().unwrap_or(0.0));
        }
        if start > 0 {
            self.cmd(vec!["set_property".into(), "playlist-pos".into(), (start as i64).into()]);
        }
        self.cmd(vec!["set_property".into(), "pause".into(), false.into()]);
    }

    pub fn enqueue(&mut self, urls: &[String], resume: &[f64]) {
        let idle = self.state.lock().unwrap().idle;
        for (i, u) in urls.iter().enumerate() {
            let mode = if idle && i == 0 { "append-play" } else { "append" };
            self.load(u, mode, resume.get(i).copied().unwrap_or(0.0));
        }
    }

    pub fn toggle_pause(&mut self) {
        self.cmd(vec!["cycle".into(), "pause".into()]);
    }
    pub fn set_pause(&mut self, paused: bool) {
        self.cmd(vec!["set_property".into(), "pause".into(), paused.into()]);
    }
    pub fn seek_to(&mut self, secs: f64) {
        self.cmd(vec!["seek".into(), secs.into(), "absolute".into()]);
    }
    pub fn next(&mut self) {
        self.cmd(vec!["playlist-next".into()]);
    }
    pub fn prev(&mut self) {
        // Restart the track if we are past the first few seconds.
        if self.state.lock().unwrap().pos > 5.0 {
            self.cmd(vec!["seek".into(), 0.into(), "absolute".into()]);
        } else {
            self.cmd(vec!["playlist-prev".into()]);
        }
    }
    pub fn seek(&mut self, secs: i64) {
        self.cmd(vec!["seek".into(), secs.into(), "relative".into()]);
    }
    pub fn volume(&mut self, delta: i64) {
        self.cmd(vec!["add".into(), "volume".into(), delta.into()]);
    }
    pub fn stop(&mut self) {
        self.cmd(vec!["playlist-clear".into()]);
        self.cmd(vec!["stop".into()]);
    }
    pub fn jump(&mut self, index: usize) {
        self.cmd(vec!["set_property".into(), "playlist-pos".into(), (index as i64).into()]);
        self.cmd(vec!["set_property".into(), "pause".into(), false.into()]);
    }
    pub fn remove(&mut self, index: usize) {
        self.cmd(vec!["playlist-remove".into(), (index as i64).into()]);
    }

    /// False once the mpv process has exited (crash, or closed by the user).
    pub fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    pub fn snapshot(&self) -> PlayerState {
        self.state.lock().unwrap().clone()
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.cmd(vec!["quit".into()]);
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.sock);
    }
}
