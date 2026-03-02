use crate::Error;
use std::{fs::File, io::Write, path::PathBuf};

const APP_NAME: &str = "vimcord";

pub enum LogType {
    Error,
    #[allow(dead_code)]
    Warning,
    #[allow(dead_code)]
    Info,
    #[allow(dead_code)]
    Debug,
}

fn write_log_file(path: PathBuf, msg: &[u8]) -> Result<(), Error> {
    File::options()
        .append(true)
        .create(true)
        .open(path)?
        .write_all(msg)?;
    Ok(())
}

pub fn print_log(msg: Error, log_type: LogType) -> Result<(), Error> {
    let timestamp = chrono::offset::Local::now();
    let type_str = match log_type {
        LogType::Error => "ERROR",
        LogType::Warning => "WARN",
        LogType::Info => "INFO",
        LogType::Debug => "DEBUG",
    };
    let msg = format!("[{timestamp}] {type_str}: {msg}\n");
    let mut path = get_log_directory(APP_NAME).unwrap_or(".".into());
    let _ = std::fs::create_dir_all(&path);
    path.push("logs");

    write_log_file(path, msg.as_bytes())?;
    Ok(())
}

fn get_log_directory(app_name: &str) -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        // Windows: %LOCALAPPDATA%\vimcord\logs
        dirs::data_local_dir().map(|mut path| {
            path.push(app_name);
            path
        })
    }

    #[cfg(target_os = "macos")]
    {
        // macOS: ~/Library/Logs/vimcord/logs
        dirs::home_dir().map(|mut path| {
            path.push("Library");
            path.push("Logs");
            path.push(app_name);
            path
        })
    }

    #[cfg(target_os = "linux")]
    {
        // Linux: ~/.cache/vimcord/logs
        let mut path = dirs::cache_dir()?;

        path.push(app_name);
        Some(path)
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        std::env::current_dir().ok()
    }
}
