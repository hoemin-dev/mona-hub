use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::PathBuf,
};

const DIAGNOSTIC_FILE_NAME: &str = "acdc-diagnostic.log";

fn diagnostic_path() -> io::Result<PathBuf> {
    let local_app_data = std::env::var_os("LOCALAPPDATA")
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "LOCALAPPDATA is not available"))?;
    Ok(PathBuf::from(local_app_data)
        .join("Mona")
        .join("mona-hub")
        .join(DIAGNOSTIC_FILE_NAME))
}

pub fn append(message: &str) -> io::Result<()> {
    let path = diagnostic_path()?;
    fs::create_dir_all(path.parent().expect("diagnostic path always has a parent"))?;
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(
        file,
        "{} {message}",
        chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
    )
}

pub fn record(message: &str) {
    if let Err(error) = append(message) {
        log::warn!("[AC/DC-DIAG] diagnostic file write failed: {error}");
    }
}
