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

// --- EXPORT FUNCTIONS ---

pub fn get_exports_dir() -> PathBuf {
    let config_dir = dirs::config_dir()
        .unwrap_or_default()
        .join("xt-screen-translator")
        .join("exports");
    let _ = std::fs::create_dir_all(&config_dir);
    config_dir
}

pub fn export_to_txt(entry: &HistoryEntry) -> Result<PathBuf, String> {
    let exports_dir = get_exports_dir();
    let filename = format!("{}_{}.txt", entry.preset_name.replace(" ", "_"), entry.id);
    let path = exports_dir.join(&filename);
    
    let content = format!(
        "Preset: {}\nType: {}\nTime: {}\n\n---\n\n{}",
        entry.preset_name,
        entry.preset_type,
        format_timestamp(entry.timestamp),
        entry.result_text
    );
    
    std::fs::write(&path, content).map_err(|e| e.to_string())?;
    Ok(path)
}

pub fn export_to_markdown(entry: &HistoryEntry) -> Result<PathBuf, String> {
    let exports_dir = get_exports_dir();
    let filename = format!("{}_{}.md", entry.preset_name.replace(" ", "_"), entry.id);
    let path = exports_dir.join(&filename);
    
    let star = if entry.is_favorite { " ⭐" } else { "" };
    let content = format!(
        "# {}{}\n\n**Type:** {}  \n**Time:** {}\n\n---\n\n{}\n",
        entry.preset_name,
        star,
        entry.preset_type,
        format_timestamp(entry.timestamp),
        entry.result_text
    );
    
    std::fs::write(&path, content).map_err(|e| e.to_string())?;
    Ok(path)
}

pub fn format_for_clipboard(entry: &HistoryEntry) -> String {
    format!(
        "[{}] {}\n\n{}",
        entry.preset_name,
        format_timestamp(entry.timestamp),
        entry.result_text
    )
}

fn format_timestamp(timestamp: u64) -> String {
    let local_ts = timestamp + 7 * 3600;
    let secs_per_day = 86400u64;
    let secs_per_hour = 3600u64;
    let secs_per_min = 60u64;
    
    let days_since_epoch = local_ts / secs_per_day;
    let remainder = local_ts % secs_per_day;
    let hour = remainder / secs_per_hour;
    let minute = (remainder % secs_per_hour) / secs_per_min;
    
    let mut year = 1970u64;
    let mut remaining_days = days_since_epoch;
    
    loop {
        let days_in_year = if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) { 366 } else { 365 };
        if remaining_days < days_in_year { break; }
        remaining_days -= days_in_year;
        year += 1;
    }
    
    let days_in_months = if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    
    let mut month = 1u64;
    for &days in &days_in_months {
        if remaining_days < days { break; }
        remaining_days -= days;
        month += 1;
    }
    let day = remaining_days + 1;
    
    format!("{:02}/{:02}/{} {:02}:{:02}", day, month, year, hour, minute)
}
