use std::path::PathBuf;

fn token_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    Some(home.join(".juiceboxplus").join("token"))
}

pub fn load_token() -> Option<String> {
    let path = token_path()?;
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

pub fn save_token(token: &str) -> Result<(), String> {
    let home = dirs::home_dir().ok_or("Cannot find home directory")?;
    let token_dir = home.join(".juiceboxplus");
    std::fs::create_dir_all(&token_dir).map_err(|e| format!("Failed to create dir: {e}"))?;
    let token_path = token_dir.join("token");
    {
        use std::fs::OpenOptions;
        use std::io::Write;
        #[cfg(unix)]
        use std::os::unix::fs::OpenOptionsExt;
        let mut opts = OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        opts.mode(0o600);
        let mut file = opts
            .open(&token_path)
            .map_err(|e| format!("Failed to open token file: {e}"))?;
        file.write_all(token.as_bytes())
            .map_err(|e| format!("Failed to write token: {e}"))
    }
}
