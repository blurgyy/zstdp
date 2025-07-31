use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const MIME_DB_JSON: &str = include_str!(env!("MIME_DB_PATH"));

#[derive(Debug, Deserialize, Serialize)]
pub struct MimeEntry {
    pub source: Option<String>,
    pub charset: Option<String>,
    pub compressible: Option<bool>,
    pub extensions: Option<Vec<String>>,
}

pub type MimeDatabase = HashMap<String, MimeEntry>;

#[derive(Debug, Clone)]
pub enum CompressionPolicy {
    Strict,
    Permissive,
}

#[derive(Debug, Clone)]
pub struct MimeConfig {
    pub compression_policy: CompressionPolicy,
    pub additional_compressible: Vec<String>,
    pub excluded_types: Vec<String>,
    pub custom_list: Option<Vec<String>>,
}

impl Default for MimeConfig {
    fn default() -> Self {
        Self {
            compression_policy: CompressionPolicy::Strict,
            additional_compressible: Vec::new(),
            excluded_types: Vec::new(),
            custom_list: None,
        }
    }
}

pub struct MimeChecker {
    database: MimeDatabase,
    config: MimeConfig,
}

impl MimeChecker {
    pub fn new(config: MimeConfig) -> Result<Self, serde_json::Error> {
        let database: MimeDatabase = serde_json::from_str(MIME_DB_JSON)?;
        Ok(Self { database, config })
    }

    pub fn should_compress(&self, mime_type: &str) -> bool {
        // If using a custom list, only compress types in that list
        if let Some(ref custom_list) = self.config.custom_list {
            return custom_list.contains(&mime_type.to_string());
        }

        // Check if explicitly excluded
        if self.config.excluded_types.contains(&mime_type.to_string()) {
            return false;
        }

        // Check if explicitly included in additional compressible types
        if self
            .config
            .additional_compressible
            .contains(&mime_type.to_string())
        {
            return true;
        }

        // Check database entry
        if let Some(entry) = self.database.get(mime_type) {
            match entry.compressible {
                Some(true) => true,
                Some(false) => false,
                None => match self.config.compression_policy {
                    CompressionPolicy::Strict => false,
                    CompressionPolicy::Permissive => true,
                },
            }
        } else {
            // Unknown MIME type
            match self.config.compression_policy {
                CompressionPolicy::Strict => false,
                CompressionPolicy::Permissive => true,
            }
        }
    }

    pub fn get_compressible_types(&self) -> Vec<String> {
        self.database
            .iter()
            .filter_map(|(mime_type, entry)| {
                if entry.compressible == Some(true) {
                    Some(mime_type.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn get_non_compressible_types(&self) -> Vec<String> {
        self.database
            .iter()
            .filter_map(|(mime_type, entry)| {
                if entry.compressible == Some(false) {
                    Some(mime_type.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn get_unspecified_types(&self) -> Vec<String> {
        self.database
            .iter()
            .filter_map(|(mime_type, entry)| {
                if entry.compressible.is_none() {
                    Some(mime_type.clone())
                } else {
                    None
                }
            })
            .collect()
    }
}
