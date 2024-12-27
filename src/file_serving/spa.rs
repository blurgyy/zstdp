use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct SpaConfig {
    pub index_path: PathBuf,
    pub static_extensions: HashSet<String>,
}

impl Default for SpaConfig {
    fn default() -> Self {
        let mut static_extensions = HashSet::new();
        // Common static file extensions - all lowercase
        for ext in &[
            "css", "js", "jpg", "jpeg", "png", "gif", "svg", "ico", "woff", "woff2", "ttf", "eot",
            "pdf", "json", "webp", "map", "txt",
        ] {
            static_extensions.insert(ext.to_string());
        }

        Self {
            index_path: PathBuf::from("index.html"),
            static_extensions,
        }
    }
}

impl SpaConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_static_file(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| self.static_extensions.contains(&ext.to_lowercase()))
            .unwrap_or(false)
    }
}
