use std::env;
use std::fs;
use std::path::Path;

fn main() {
    let mime_db_path =
        env::var("MIME_DB_PATH").expect("MIME_DB_PATH environment variable must be set");

    let mime_db_file = Path::new(&mime_db_path);

    if !mime_db_file.exists() {
        panic!(
            "MIME database file not found at: {}",
            mime_db_file.display()
        );
    }

    // The MIME_DB_PATH points directly to the JSON file (from Nix build)
    let json_file = mime_db_file;

    let out_dir = env::var("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("mime_db.rs");

    // Use include_str! to include the JSON file at compile time
    fs::write(
        dest_path,
        format!(
            r#"
pub const MIME_DB_JSON: &str = include_str!("{}");
"#,
            json_file.display()
        ),
    )
    .expect("Failed to write MIME database module");

    println!("cargo:rerun-if-changed={}", json_file.display());
    println!("cargo:rerun-if-env-changed=MIME_DB_PATH");
}
