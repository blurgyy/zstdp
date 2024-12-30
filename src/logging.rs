use env_logger::Builder;
use log::LevelFilter;
use std::io::Write;
use std::path::Path;
use std::time::SystemTime;

pub fn setup_logging() {
    Builder::new()
        .filter_level(LevelFilter::Info) // Set default level
        .parse_env("RUST_LOG") // Allow override through env var
        .format(|buf, record| {
            let timestamp = SystemTime::now();
            let level = record.level();

            if atty::is(atty::Stream::Stderr) {
                // Terminal output with colors
                let level_color = match level {
                    log::Level::Error => "\x1B[31m", // Red
                    log::Level::Warn => "\x1B[33m",  // Yellow
                    log::Level::Info => "\x1B[32m",  // Green
                    log::Level::Debug => "\x1B[36m", // Cyan
                    log::Level::Trace => "\x1B[35m", // Magenta
                };

                // Only include file and line for debug/trace levels
                if level <= log::Level::Debug {
                    writeln!(
                        buf,
                        "{}{:>5}\x1B[0m [{}] {} - {}:{}",
                        level_color,
                        level,
                        humantime::format_rfc3339_millis(timestamp),
                        record.args(),
                        record.file().unwrap_or("unknown"),
                        record.line().unwrap_or(0)
                    )
                } else {
                    writeln!(
                        buf,
                        "{}{:>5}\x1B[0m [{}] {}",
                        level_color,
                        level,
                        humantime::format_rfc3339_millis(timestamp),
                        record.args()
                    )
                }
            } else {
                // Plain output for non-terminal
                if level <= log::Level::Debug {
                    writeln!(
                        buf,
                        "{:>5} [{}] {} - {}:{}",
                        level,
                        humantime::format_rfc3339_millis(timestamp),
                        record.args(),
                        record.file().unwrap_or("unknown"),
                        record.line().unwrap_or(0)
                    )
                } else {
                    writeln!(
                        buf,
                        "{:>5} [{}] {}",
                        level,
                        humantime::format_rfc3339_millis(timestamp),
                        record.args()
                    )
                }
            }
        })
        .init();
}

#[macro_export]
macro_rules! log_request {
    ($request:expr) => {{
        let parts: Vec<&str> = $request.trim().split_whitespace().collect();
        if parts.len() >= 2 {
            log::info!("→ {} {}", parts[0], parts[1])
        } else {
            log::info!("→ Invalid request format: {}", $request.trim())
        }
    }};
}

#[macro_export]
macro_rules! log_response {
    ($status:expr, $duration:expr, $original_size:expr, $final_size:expr) => {
        log::info!(
            "← {} ({:?}) - Size: {} → {}",
            $status,
            $duration,
            $original_size,
            $final_size
        )
    };
}

#[macro_export]
macro_rules! log_error {
    ($error:expr, $context:expr) => {
        log::error!("❌ {} - {}", $context, $error)
    };
}

// Trait for types that can be logged
pub trait Loggable {
    fn log_description(&self) -> String;
}

// Implement for types that implement Display
impl<T: std::fmt::Display> Loggable for T {
    fn log_description(&self) -> String {
        self.to_string()
    }
}

// Special implementation for Path
impl Loggable for Path {
    fn log_description(&self) -> String {
        self.display().to_string()
    }
}

pub trait LoggingExt: Loggable {
    fn log_operation<F, T, E>(&self, operation: &str, f: F) -> Result<T, E>
    where
        F: FnOnce() -> Result<T, E>,
        E: std::fmt::Display;
}

impl<S: ?Sized + Loggable> LoggingExt for S {
    fn log_operation<F, T, E>(&self, operation: &str, f: F) -> Result<T, E>
    where
        F: FnOnce() -> Result<T, E>,
        E: std::fmt::Display,
    {
        log::debug!("Starting {} on {}", operation, self.log_description());
        match f() {
            Ok(result) => {
                log::debug!("Completed {} on {}", operation, self.log_description());
                Ok(result)
            }
            Err(e) => {
                log::error!("Failed {} on {}: {}", operation, self.log_description(), e);
                Err(e)
            }
        }
    }
}
