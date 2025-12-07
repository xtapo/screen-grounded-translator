use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct HistoryEntry {
    pub id: String,
    pub preset_name: String,
    pub preset_type: String,  // "image" or "audio"
    pub input_summary: String,
    pub result_text: String,
    pub retrans_text: Option<String>,
    pub timestamp: u64,
    pub is_favorite: bool,
}

lazy_static::lazy_static! {
    static ref HISTORY_CACHE: Mutex<Vec<HistoryEntry>> = Mutex::new(Vec::new());
    static ref HISTORY_LOADED: Mutex<bool> = Mutex::new(false);
}

const MAX_HISTORY_ENTRIES: usize = 100;

pub fn get_history_path() -> PathBuf {
    let config_dir = dirs::config_dir()
        .unwrap_or_default()
        .join("xt-screen-translator");
    let _ = std::fs::create_dir_all(&config_dir);
    config_dir.join("history.json")
}

pub fn load_history() -> Vec<HistoryEntry> {
    let mut loaded = HISTORY_LOADED.lock().unwrap();
    if *loaded {
        return HISTORY_CACHE.lock().unwrap().clone();
    }
    
    let path = get_history_path();
    let entries = if path.exists() {
        let data = std::fs::read_to_string(&path).unwrap_or_default();
        serde_json::from_str(&data).unwrap_or_default()
    } else {
        Vec::new()
    };
    
    *HISTORY_CACHE.lock().unwrap() = entries.clone();
    *loaded = true;
    entries
}

pub fn save_history(entries: &[HistoryEntry]) {
    let path = get_history_path();
    if let Ok(data) = serde_json::to_string_pretty(entries) {
        let _ = std::fs::write(path, data);
    }
    *HISTORY_CACHE.lock().unwrap() = entries.to_vec();
}

pub fn add_history_entry(entry: HistoryEntry) {
    let mut entries = load_history();
    
    // Add to beginning (newest first)
    entries.insert(0, entry);
    
    // Limit history size
    if entries.len() > MAX_HISTORY_ENTRIES {
        entries.truncate(MAX_HISTORY_ENTRIES);
    }
    
    save_history(&entries);
}

pub fn toggle_favorite(id: &str) {
    let mut entries = load_history();
    if let Some(entry) = entries.iter_mut().find(|e| e.id == id) {
        entry.is_favorite = !entry.is_favorite;
    }
    save_history(&entries);
}

pub fn delete_entry(id: &str) {
    let mut entries = load_history();
    entries.retain(|e| e.id != id);
    save_history(&entries);
}

pub fn clear_all_history() {
    save_history(&[]);
}

pub fn generate_entry_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{:x}", now)
}

pub fn get_current_timestamp() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}
