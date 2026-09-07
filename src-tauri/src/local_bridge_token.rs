use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::PathBuf,
    sync::OnceLock,
};

const TOKEN_FILE_NAME: &str = "ac-dc-local-bridge.token";
const TOKEN_BYTES: usize = 32;
static TOKEN: OnceLock<String> = OnceLock::new();

fn token_path() -> io::Result<PathBuf> {
    let local_app_data = std::env::var_os("LOCALAPPDATA")
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "LOCALAPPDATA is not available"))?;
    Ok(PathBuf::from(local_app_data)
        .join("Mona")
        .join(TOKEN_FILE_NAME))
}

fn validate(value: &str) -> io::Result<String> {
    let token = value.trim();
    if token.len() < 32 || !token.is_ascii() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "the local bridge token file is invalid",
        ));
    }
    Ok(token.to_owned())
}

fn load_or_create_uncached() -> io::Result<String> {
    let path = token_path()?;
    match fs::read_to_string(&path) {
        Ok(value) => return validate(&value),
        Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
        Err(_) => {}
    }

    let mut bytes = [0_u8; TOKEN_BYTES];
    getrandom::fill(&mut bytes)
        .map_err(|error| io::Error::other(format!("secure token generation failed: {error}")))?;
    let token: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();

    let parent = path.parent().expect("token path always has a parent");
    fs::create_dir_all(parent)?;
    match OpenOptions::new().write(true).create_new(true).open(&path) {
        Ok(mut file) => {
            file.write_all(token.as_bytes())?;
            file.sync_all()?;
            Ok(token)
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            validate(&fs::read_to_string(path)?)
        }
        Err(error) => Err(error),
    }
}

pub fn load_or_create() -> io::Result<String> {
    if let Some(token) = TOKEN.get() {
        return Ok(token.clone());
    }
    let token = load_or_create_uncached()?;
    let _ = TOKEN.set(token);
    Ok(TOKEN
        .get()
        .expect("local bridge token was initialized")
        .clone())
}

pub fn initialize() -> io::Result<()> {
    load_or_create().map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::validate;

    #[test]
    fn accepts_persisted_token_with_trailing_newline() {
        let token = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        assert_eq!(validate(&format!("{token}\n")).unwrap(), token);
    }

    #[test]
    fn rejects_short_or_non_ascii_tokens() {
        assert!(validate("too-short").is_err());
        assert!(validate(&"가".repeat(32)).is_err());
    }
}
