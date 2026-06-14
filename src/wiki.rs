use std::fs;
use std::path::{Path, PathBuf};
use std::collections::HashMap;
use std::cell::RefCell;

pub struct WikiManager {
    root_dir: PathBuf,
    registry: RefCell<HashMap<String, Vec<String>>>,
}

impl WikiManager {
    pub fn new<P: AsRef<Path>>(root: P) -> Self {
        let root_dir = root.as_ref().to_path_buf();
        let registry_path = root_dir.join(".calki-links.json");
        let mut registry_map = HashMap::new();

        if registry_path.exists() {
            if let Ok(content) = fs::read_to_string(&registry_path) {
                if let Ok(map) = serde_json::from_str(&content) {
                    registry_map = map;
                }
            }
        } else if root_dir.exists() {
            if let Ok(entries) = fs::read_dir(&root_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("md") {
                        if let Some(file_name) = path.file_name().and_then(|s| s.to_str()) {
                            let outgoing = Self::parse_outgoing_links(&path);
                            registry_map.insert(file_name.to_string(), outgoing);
                        }
                    }
                }
            }
            if !registry_map.is_empty() {
                if let Ok(content) = serde_json::to_string_pretty(&registry_map) {
                    let _ = fs::write(&registry_path, content);
                }
            }
        }

        Self {
            root_dir,
            registry: RefCell::new(registry_map),
        }
    }

    pub fn root_dir(&self) -> &Path {
        &self.root_dir
    }

    // Ensures the wiki directory and home.md onboarding file exist
    pub fn init_wiki(&self) -> Result<PathBuf, String> {
        if !self.root_dir.exists() {
            fs::create_dir_all(&self.root_dir)
                .map_err(|e| format!("Failed to create wiki directory: {}", e))?;
        }

        let home_path = self.root_dir.join("home.md");
        let legacy_index_path = self.root_dir.join("index.md");
        if legacy_index_path.exists() && !home_path.exists() {
            let _ = fs::rename(&legacy_index_path, &home_path);
        }

        let grocery_path = self.root_dir.join("grocery-list.md");
        let savings_path = self.root_dir.join("savings-plan.md");
        let trip_path = self.root_dir.join("trip-planning.md");

        if !home_path.exists() {
            let onboarding_content = r#"# Welcome to calki! 🧮 📝

calki is a terminal-based Markdown note editor and interactive math sheet calculator with local wiki-style link navigation.

## 1. Editing Modes & Exiting
calki uses Vim-style modal editing. You can navigate the editor using standard Vim motions (`h`/`j`/`k`/`l`).
* **Switch to Insert Mode**: Press **`i`** to type text and equations.
* **Switch to Visual Mode**: Press **`v`** to select/highlight text.
* **Return to Normal Mode**: Press **`Esc`** at any time.
* **Exit the Program**: In Normal Mode, press **`Ctrl-q`** or **`ZZ`** to save your work and exit.

## 2. Interactive Math Sheets
Write variables and equations, ending evaluation lines with `=>`. Values are calculated in real time when you exit Insert mode (`Esc`).

price = 100
tax_rate = 8.5%
quantity = 5

Let's calculate the total:
price * quantity * (1 + tax_rate) => 542.5

We can also write calculations inline: `price * quantity => 500` before tax.

## 3. Dynamic Wiki Links & Creating Pages
Notes can be linked together using double square brackets like `[[Grocery List]]`.
* **Follow links**: Place your cursor over a link and press **Enter** in Normal mode to jump to it.
* **Go back**: Press **Backspace** or **Ctrl-o** to return in history.
* **Create links**: In Visual mode, highlight any text and press **Enter** to instantly wrap it in a wiki link.
* **Create new pages**: Simply write a new link name (e.g. `[[My New Project]]`) and press **Enter** over it. `calki` will automatically create the new page and open it for editing!

## 4. Sample Sheets
We've pre-generated a few demo notes to showcase different capabilities. Press **Enter** on these links to explore:
* **Budgeting & Quantities**: [[Grocery List]]
* **Financial Forecasting**: [[Savings Plan]]
* **Unit Conversions & Speed**: [[Trip Planning]]

## 5. Sidebar Panels
* Press **F2** to toggle the left **Wiki Map** (shows backlinks and references).
* Press **F3** to toggle the right **Variables Inspector** (shows active scope values).
* Press **Ctrl-h** / **Ctrl-l** to switch focus between active panels.
"#;
            fs::write(&home_path, onboarding_content)
                .map_err(|e| format!("Failed to write onboarding home.md: {}", e))?;

            // Generate sample pages if they don't exist
            if !grocery_path.exists() {
                let grocery_content = r#"# Grocery List 🛒

Planning this week's groceries and budgeting with tax and discounts.

## Items & Prices
apples = 6 * $0.75 => $4.50
milk = 2 * $3.29 => $6.58
bread = 1 * $2.49 => $2.49
cheese = 0.5 kg * $12.00 / kg => $6.00

## Calculations
subtotal = apples + milk + bread + cheese
subtotal => $19.57

## Tax & Discounts
discount = 10%
coupon_savings = subtotal * discount => $1.957
tax_rate = 8.5%

## Final Bill
total = (subtotal - coupon_savings) * (1 + tax_rate)
total => $19.1091

Back to [[Home]].
"#;
                let _ = fs::write(&grocery_path, grocery_content);
            }

            if !savings_path.exists() {
                let savings_content = r#"# Savings Plan 💰

Let's plan for a big purchase or retirement using financial compounding functions.

## Goals & Variables
target = $50000
initial_deposit = $5000
monthly_contribution = $450
annual_rate = 6%
years = 5

## Calculations
months = years * 12 => 60
monthly_rate = annual_rate / 12 => 0.005

## Future Value
# fv(rate, nper, pmt, pv) calculates future value of an investment
future_value = fv(monthly_rate, months, -1 * monthly_contribution, -1 * initial_deposit)
future_value => $39502.82

## Gap to Target
shortfall = target - future_value
shortfall => $10497.18

Required monthly boost to hit the target:
additional_pmt = shortfall * monthly_rate / (1 - (1 + monthly_rate)^(-1 * months)) => $148.96

Back to [[Home]].
"#;
                let _ = fs::write(&savings_path, savings_content);
            }

            if !trip_path.exists() {
                let trip_content = r#"# Trip Planning 🚗 ✈️

Calculating driving times, fuel costs, and speed conversions for a road trip.

## Route Details
distance = 320 miles
speed_limit = 65 mph
fuel_efficiency = 28 miles / gallon
gas_price = $3.89 / gallon

## Fuel Calculation
fuel_needed = distance / fuel_efficiency
fuel_needed => 11.4286 gallon

total_gas_cost = fuel_needed * gas_price
total_gas_cost => $44.4571

## Duration
driving_time = distance / speed_limit
driving_time in hours => 4.9231 hours

## Metric Conversions
metric_distance = distance to km
metric_distance => 514.99 km

metric_speed = speed_limit to km/h
metric_speed => 104.6074 km/h

Back to [[Home]].
"#;
                let _ = fs::write(&trip_path, trip_content);
            }
        }

        // Scan existing files so registry starts populated
        if home_path.exists() {
            self.scan_outgoing_links(&home_path);
        }
        if grocery_path.exists() {
            self.scan_outgoing_links(&grocery_path);
        }
        if savings_path.exists() {
            self.scan_outgoing_links(&savings_path);
        }
        if trip_path.exists() {
            self.scan_outgoing_links(&trip_path);
        }

        Ok(home_path)
    }

    // Converts a link name (e.g. "Project Ideas") to a file path ("project-ideas.md")
    pub fn link_to_path(&self, link_name: &str) -> PathBuf {
        let clean_name = link_name
            .trim()
            .to_lowercase()
            .replace(|c: char| !c.is_alphanumeric() && c != ' ', "")
            .replace(' ', "-");
        self.root_dir.join(format!("{}.md", clean_name))
    }

    // Converts a file path back to a human-readable title (e.g. "project-ideas.md" -> "Project Ideas")
    pub fn path_to_title(&self, path: &Path) -> String {
        if let Some(stem) = path.file_stem().and_string_lossy() {
            if stem == "index" || stem == "home" {
                return "Home".to_string();
            }
            // Title case: replace hyphens with spaces and capitalize words
            stem.split('-')
                .map(|word| {
                    let mut chars = word.chars();
                    match chars.next() {
                        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                        None => String::new(),
                    }
                })
                .collect::<Vec<String>>()
                .join(" ")
        } else {
            "Untitled".to_string()
        }
    }

    // Updates the registry entry for a file, saving it to disk
    pub fn update_registry_entry(&self, file_path: &Path, outgoing: Vec<String>) {
        if let Some(file_name) = file_path.file_name().and_then(|s| s.to_str()) {
            self.registry.borrow_mut().insert(file_name.to_string(), outgoing);
            let registry_path = self.root_dir.join(".calki-links.json");
            if let Ok(content) = serde_json::to_string_pretty(&*self.registry.borrow()) {
                let _ = fs::write(&registry_path, content);
            }
        }
    }

    // Removes the registry entry for a file, saving it to disk
    pub fn remove_registry_entry(&self, file_path: &Path) {
        if let Some(file_name) = file_path.file_name().and_then(|s| s.to_str()) {
            self.registry.borrow_mut().remove(file_name);
            let registry_path = self.root_dir.join(".calki-links.json");
            if let Ok(content) = serde_json::to_string_pretty(&*self.registry.borrow()) {
                let _ = fs::write(&registry_path, content);
            }
        }
    }

    // Parses a specific file for outgoing wiki links: [[Link Name]] without registry side-effects
    fn parse_outgoing_links(file_path: &Path) -> Vec<String> {
        let mut links = Vec::new();
        let content = match fs::read_to_string(file_path) {
            Ok(txt) => txt,
            Err(_) => return links,
        };

        let mut chars = content.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == '[' && chars.peek() == Some(&'[') {
                chars.next(); // consume second '['
                let mut link_name = String::new();
                let mut closed = false;
                while let Some(next_ch) = chars.next() {
                    if next_ch == ']' && chars.peek() == Some(&']') {
                        chars.next(); // consume second ']'
                        closed = true;
                        break;
                    }
                    link_name.push(next_ch);
                }
                if closed {
                    let cleaned = link_name.trim().to_string();
                    if !cleaned.is_empty() && !links.contains(&cleaned) {
                        links.push(cleaned);
                    }
                }
            }
        }
        links
    }

    // Scans a specific file for outgoing wiki links: [[Link Name]]
    pub fn scan_outgoing_links(&self, file_path: &Path) -> Vec<String> {
        let links = Self::parse_outgoing_links(file_path);
        self.update_registry_entry(file_path, links.clone());
        links
    }

    // Scans all files in the wiki directory to see which ones contain a link to the target path
    pub fn scan_backlinks(&self, target_path: &Path) -> Vec<String> {
        let mut backlinks = Vec::new();
        let target_title = self.path_to_title(target_path).to_lowercase();
        let target_file_name = target_path.file_name().and_then(|s| s.to_str()).unwrap_or("");

        let registry = self.registry.borrow();
        for (file_name, outgoing) in registry.iter() {
            // Don't backlink to itself
            if let Some(target_name) = target_path.file_name().and_then(|s| s.to_str()) {
                if file_name == target_name {
                    continue;
                }
            }

            let is_referenced = outgoing.iter().any(|link| {
                let linked_path = self.link_to_path(link);
                let linked_file_name = linked_path.file_name().and_then(|s| s.to_str()).unwrap_or("");
                linked_file_name == target_file_name || link.to_lowercase() == target_title
            });

            if is_referenced {
                let path = self.root_dir.join(file_name);
                backlinks.push(self.path_to_title(&path));
            }
        }

        backlinks
    }
}

// Trait extension helper for Option<&OsStr> mapping
trait OsStrExt {
    fn and_string_lossy(&self) -> Option<String>;
}
impl OsStrExt for Option<&std::ffi::OsStr> {
    fn and_string_lossy(&self) -> Option<String> {
        self.map(|s| s.to_string_lossy().into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_resolutions() {
        let mgr = WikiManager::new("/tmp/calki-test-wiki");
        let path = mgr.link_to_path("Project Ideas");
        assert!(path.to_string_lossy().ends_with("project-ideas.md"));

        let title = mgr.path_to_title(&PathBuf::from("/tmp/calki-test-wiki/project-ideas.md"));
        assert_eq!(title, "Project Ideas");

        let index_title = mgr.path_to_title(&PathBuf::from("/tmp/calki-test-wiki/index.md"));
        assert_eq!(index_title, "Home");

        let home_title = mgr.path_to_title(&PathBuf::from("/tmp/calki-test-wiki/home.md"));
        assert_eq!(home_title, "Home");
    }

    #[test]
    fn test_wiki_link_registry() {
        let temp_dir = std::env::current_dir().unwrap().join("test_wiki_temp_registry");
        if temp_dir.exists() {
            let _ = fs::remove_dir_all(&temp_dir);
        }
        fs::create_dir_all(&temp_dir).unwrap();

        // 1. Create a few notes
        let note1 = temp_dir.join("home.md");
        fs::write(&note1, "# Home\nSee [[Grocery List]]").unwrap();
        let note2 = temp_dir.join("grocery-list.md");
        fs::write(&note2, "# Grocery List\nGo back to [[Home]]").unwrap();

        // 2. Initialize manager and scan
        let mgr = WikiManager::new(&temp_dir);
        // Explicitly trigger scan by loading registry
        assert_eq!(mgr.registry.borrow().len(), 2);
        assert!(mgr.registry.borrow().contains_key("home.md"));
        assert!(mgr.registry.borrow().contains_key("grocery-list.md"));

        // 3. Test backlinks query using registry
        let backlinks = mgr.scan_backlinks(&note2);
        assert_eq!(backlinks, vec!["Home"]);

        // 4. Update note
        fs::write(&note1, "# Home\nNo more links").unwrap();
        mgr.scan_outgoing_links(&note1); // updates registry

        let backlinks_updated = mgr.scan_backlinks(&note2);
        assert!(backlinks_updated.is_empty());

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_init_wiki_conditional_onboarding() {
        let temp_dir = std::env::current_dir().unwrap().join("test_wiki_temp_conditional");
        if temp_dir.exists() {
            let _ = fs::remove_dir_all(&temp_dir);
        }
        fs::create_dir_all(&temp_dir).unwrap();

        // Write home.md beforehand
        let home_path = temp_dir.join("home.md");
        fs::write(&home_path, "# Custom Home\n").unwrap();

        let mgr = WikiManager::new(&temp_dir);
        let init_res = mgr.init_wiki();
        assert!(init_res.is_ok());

        // The custom home should not be overwritten
        let home_content = fs::read_to_string(&home_path).unwrap();
        assert_eq!(home_content, "# Custom Home\n");

        // The default sample files (grocery-list.md, etc.) should not be generated
        assert!(!temp_dir.join("grocery-list.md").exists());
        assert!(!temp_dir.join("savings-plan.md").exists());
        assert!(!temp_dir.join("trip-planning.md").exists());

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
