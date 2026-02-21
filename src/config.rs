use serde::{Deserialize, Serialize};

const DEFAULT_EMOJIS_JSON: &str = include_str!("../emojis.json");

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Config {
    pub version: u8,
    #[serde(default)]
    pub vim_mode: bool,
    #[serde(default)]
    pub discreet_notifs: bool,
    pub emoji_map: Vec<(String, String)>,
}

fn load_emojis() -> Vec<(String, String)> {
    match serde_json::from_str::<Vec<(String, String)>>(DEFAULT_EMOJIS_JSON) {
        Ok(map) => map,
        Err(e) => {
            eprintln!("Error parsing emojis dictionary: {e}");
            Vec::new()
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: 1,
            vim_mode: true,
            discreet_notifs: false,
            emoji_map: Vec::new(),
        }
    }
}

pub fn load_config() -> Config {
    let app_name = "rivetui";
    match confy::load::<Config>(app_name, "config") {
        Ok(mut cfg) => {
            if cfg.emoji_map.is_empty() {
                cfg.emoji_map = load_emojis();
                if let Err(e) = confy::store::<Config>(app_name, "config", cfg.clone()) {
                    eprintln!("Error storing config: {e}");
                }
            }
            cfg
        }
        Err(e) => {
            eprintln!("Error loading config: {e}");
            Config::default()
        }
    }
}
