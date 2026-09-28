use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Config {
    pub server: String,
    pub user: String,
    pub token: String,
    pub user_id: String,
    pub device_id: String,
    #[serde(default)]
    pub insecure: bool,
}

fn path() -> PathBuf {
    let mut p = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    p.push("jf400");
    p.push("config.json");
    p
}

impl Config {
    pub fn load() -> Config {
        let mut cfg: Config = std::fs::read_to_string(path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        if cfg.device_id.is_empty() {
            cfg.device_id = uuid::Uuid::new_v4().to_string();
        }
        cfg
    }

    pub fn save(&self) {
        let p = path();
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(&p, s);
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600));
            }
        }
    }

    pub fn clear_session(&mut self) {
        self.token.clear();
        self.user_id.clear();
        self.save();
    }
}
