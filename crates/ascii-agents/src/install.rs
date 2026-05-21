use std::path::PathBuf;

use anyhow::Result;

pub fn install(_hook_path: Option<PathBuf>, _settings: Option<PathBuf>) -> Result<()> {
    println!("install-hooks — implemented in Phase I");
    Ok(())
}

pub fn uninstall(_settings: Option<PathBuf>) -> Result<()> {
    println!("uninstall-hooks — implemented in Phase I");
    Ok(())
}
