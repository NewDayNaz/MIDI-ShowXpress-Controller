use anyhow::Result;
use directories::ProjectDirs;
use std::fs;
use std::path::PathBuf;

use crate::models::Preset;

pub struct PresetStorage {
    file_path: PathBuf,
}

impl PresetStorage {
    pub fn new() -> Result<Self> {
        let proj_dirs = ProjectDirs::from("com", "lighting-midi", "lighting-midi-controller")
            .ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))?;

        let config_dir = proj_dirs.config_dir();
        fs::create_dir_all(config_dir)?;

        let file_path = config_dir.join("presets.json");

        Ok(Self { file_path })
    }

    pub fn load(&self) -> Result<Vec<Preset>> {
        if !self.file_path.exists() {
            return Ok(Vec::new());
        }

        let data = fs::read_to_string(&self.file_path)?;
        let presets: Vec<Preset> = serde_json::from_str(&data)?;
        Ok(presets)
    }

    pub fn save(&self, presets: &[Preset]) -> Result<()> {
        let data = serde_json::to_string_pretty(presets)?;
        fs::write(&self.file_path, data)?;
        Ok(())
    }
}

pub fn check_conflicts(presets: &[Preset], new_trigger: &crate::models::MidiTrigger) -> bool {
    presets
        .iter()
        .any(|p| p.triggers.iter().any(|t| t == new_trigger))
}
