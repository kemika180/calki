use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RatesCache {
    pub base: String,
    pub timestamp: u64,
    pub rates: HashMap<String, f64>,
}

impl Default for RatesCache {
    fn default() -> Self {
        // Fallback snapshot compiled into the binary
        let mut rates = HashMap::new();
        rates.insert("EUR".to_string(), 0.9259);
        rates.insert("GBP".to_string(), 0.7850);
        rates.insert("CAD".to_string(), 1.3650);
        rates.insert("AUD".to_string(), 1.4920);
        rates.insert("JPY".to_string(), 156.45);
        rates.insert("CNY".to_string(), 7.25);

        Self {
            base: "USD".to_string(),
            timestamp: 0, // indicates stale/hardcoded
            rates,
        }
    }
}

pub fn get_config_path() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    let mut path = PathBuf::from(home);
    path.push(".config");
    path.push("calki");
    Some(path)
}

pub fn get_rates_file_path() -> Option<PathBuf> {
    let mut path = get_config_path()?;
    path.push("rates.json");
    Some(path)
}

pub fn load_currency_rates() -> RatesCache {
    let default_cache = RatesCache::default();
    let file_path = match get_rates_file_path() {
        Some(path) => path,
        None => return default_cache,
    };

    if !file_path.exists() {
        // Try to create parent directories and write default snapshot
        if let Some(parent) = file_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(json_str) = serde_json::to_string_pretty(&default_cache) {
            let _ = fs::write(&file_path, json_str);
        }
        return default_cache;
    }

    fs::read_to_string(&file_path)
        .ok()
        .and_then(|content| serde_json::from_str::<RatesCache>(&content).ok())
        .unwrap_or(default_cache)
}

// Spawns a background thread to update exchange rates if they are older than 24 hours.
// Returns immediately, ensuring the app opens instantly.
pub fn trigger_background_update() {
    let file_path = match get_rates_file_path() {
        Some(path) => path,
        None => return,
    };

    let needs_update = fs::metadata(&file_path)
        .and_then(|m| m.modified())
        .map(|modified| {
            SystemTime::now()
                .duration_since(modified)
                .map(|elapsed| elapsed.as_secs() > 86400) // 24 hours
                .unwrap_or(true)
        })
        .unwrap_or(true);

    if needs_update {
        std::thread::spawn(move || {
            let _ = fetch_and_save_rates(file_path);
        });
    }
}

fn fetch_and_save_rates(file_path: PathBuf) -> Result<(), String> {
    let url = "https://open.er-api.com/v6/latest/USD";
    
    // Call the API with a 3-second timeout
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(3))
        .build();

    let response = agent.get(url).call()
        .map_err(|e| format!("API request failed: {}", e))?;

    let json_val: serde_json::Value = response.into_json()
        .map_err(|e| format!("Failed to parse response JSON: {}", e))?;

    let rates_obj = json_val["rates"]
        .as_object()
        .ok_or_else(|| "Missing 'rates' object in API response".to_string())?;

    let mut rates = HashMap::new();
    for (currency, val) in rates_obj {
        if let Some(rate_val) = val.as_f64() {
            rates.insert(currency.clone(), rate_val);
        }
    }

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let cache = RatesCache {
        base: "USD".to_string(),
        timestamp,
        rates,
    };

    if let Some(parent) = file_path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    let json_str = serde_json::to_string_pretty(&cache)
        .map_err(|e| format!("Failed to serialize cache: {}", e))?;

    fs::write(file_path, json_str)
        .map_err(|e| format!("Failed to write cache file: {}", e))?;

    Ok(())
}
