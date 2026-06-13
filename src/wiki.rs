use std::fs;
use std::path::{Path, PathBuf};

pub struct WikiManager {
    root_dir: PathBuf,
}

impl WikiManager {
    pub fn new<P: AsRef<Path>>(root: P) -> Self {
        Self {
            root_dir: root.as_ref().to_path_buf(),
        }
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

        if !home_path.exists() {
            let onboarding_content = r#"# Welcome to calki! 🧮 📝

calki is a text editor designed for taking notes and interactive calculations, integrated with a local wiki link structure.

## 1. How to use math sheets (Calca-style)
Write variables and equations, ending evaluation lines with `=>`. Values are calculated when you exit Insert mode (`Esc`).

price = 100
tax_rate = 8.5%
quantity = 5

Let's calculate the total:
price * quantity * (1 + tax_rate) => 542.5

You can also write calculations inline: We bought items for `price * quantity => 500` before tax.

## 2. Dynamic Wiki Links
You can link notes together using double square brackets: `[[Savings Plan]]` or `[[Project Ideas]]`.
* Place your cursor over a link and press **Enter** in Normal mode to jump to it.
* Press **Backspace** or **Ctrl-o** to return.
* Highlight text in Visual mode and press **Enter** to wrap it in a link.

## 3. Sidebar Panels
* Press **F2** to toggle the left **Wiki Map** (shows what notes link here).
* Press **F3** to toggle the right **Variables Inspector** (shows active scope values).
* Press **Ctrl-h** / **Ctrl-l** to switch focus between active panels.
"#;
            fs::write(&home_path, onboarding_content)
                .map_err(|e| format!("Failed to write onboarding home.md: {}", e))?;
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

    // Scans a specific file for outgoing wiki links: [[Link Name]]
    pub fn scan_outgoing_links(&self, file_path: &Path) -> Vec<String> {
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

    // Scans all files in the wiki directory to see which ones contain a link to the target path
    pub fn scan_backlinks(&self, target_path: &Path) -> Vec<String> {
        let mut backlinks = Vec::new();
        let target_title = self.path_to_title(target_path).to_lowercase();
        let target_file_name = target_path.file_name().and_then(|s| s.to_str()).unwrap_or("");

        let entries = match fs::read_dir(&self.root_dir) {
            Ok(iter) => iter,
            Err(_) => return backlinks,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("md") {
                // Don't backlink to itself
                if path == target_path {
                    continue;
                }

                let outgoing = self.scan_outgoing_links(&path);
                let is_referenced = outgoing.iter().any(|link| {
                    let linked_path = self.link_to_path(link);
                    let linked_file_name = linked_path.file_name().and_then(|s| s.to_str()).unwrap_or("");
                    linked_file_name == target_file_name || link.to_lowercase() == target_title
                });

                if is_referenced {
                    backlinks.push(self.path_to_title(&path));
                }
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
}
