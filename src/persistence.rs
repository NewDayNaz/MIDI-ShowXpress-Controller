use anyhow::Result;
use directories::ProjectDirs;
use std::fs;
use std::path::PathBuf;

use crate::models::Preset;
use crate::versioned_data::{load_presets, load_config, save_presets, save_config};

pub struct PresetStorage {
    file_path: PathBuf,
    config_path: PathBuf,
}

impl PresetStorage {
    pub fn new() -> Result<Self> {
        let proj_dirs = ProjectDirs::from("com", "lighting-midi", "lighting-midi-controller")
            .ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))?;

        let config_dir = proj_dirs.config_dir();
        fs::create_dir_all(config_dir)?;

        let file_path = config_dir.join("presets.json");
        let config_path = config_dir.join("config.json");

        Ok(Self { file_path, config_path })
    }

    pub fn load(&self) -> Result<Vec<Preset>> {
        if !self.file_path.exists() {
            return Ok(Vec::new());
        }

        let data = fs::read_to_string(&self.file_path)?;
        
        // Try loading with migration first
        match load_presets(&data) {
            Ok((presets, migrated_from)) => {
                // If data was migrated, save it back in the new format
                if let Some(from_version) = migrated_from {
                    eprintln!("Migrated presets from version {} to {}", from_version, crate::versioning::CURRENT_VERSION);
                    // Save the migrated data back
                    if let Err(e) = self.save(&presets) {
                        eprintln!("Warning: Failed to save migrated presets: {}", e);
                    }
                }
                Ok(presets)
            }
            Err(e) => {
                // If migration fails, try loading as unversioned (legacy format)
                eprintln!("Warning: Failed to load presets with migration: {}. Trying legacy format...", e);
                match serde_json::from_str::<Vec<Preset>>(&data) {
                    Ok(presets) => {
                        eprintln!("Successfully loaded {} presets in legacy format", presets.len());
                        // Try to save in new format
                        if let Err(save_err) = self.save(&presets) {
                            eprintln!("Warning: Failed to save presets in new format: {}", save_err);
                        }
                        Ok(presets)
                    }
                    Err(legacy_err) => {
                        Err(anyhow::anyhow!("Failed to load presets: migration error: {}, legacy format error: {}", e, legacy_err))
                    }
                }
            }
        }
    }

    pub fn save(&self, presets: &[Preset]) -> Result<()> {
        let data = save_presets(presets)?;
        fs::write(&self.file_path, data)?;
        Ok(())
    }

    pub fn load_config(&self) -> Result<AppConfig> {
        if !self.config_path.exists() {
            return Ok(AppConfig::default());
        }

        let data = fs::read_to_string(&self.config_path)?;
        let (config, migrated_from) = load_config(&data)?;
        
        // If data was migrated, save it back in the new format
        if let Some(from_version) = migrated_from {
            eprintln!("Migrated config from version {} to {}", from_version, crate::versioning::CURRENT_VERSION);
            // Save the migrated data back
            if let Err(e) = self.save_config(&config) {
                eprintln!("Warning: Failed to save migrated config: {}", e);
            }
        }
        
        Ok(config)
    }

    pub fn save_config(&self, config: &AppConfig) -> Result<()> {
        let data = save_config(config)?;
        fs::write(&self.config_path, data)?;
        Ok(())
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AppConfig {
    pub last_midi_port: Option<String>,
    pub last_controller_address: Option<String>,
    pub last_controller_password: Option<String>,
    pub last_action_type: Option<crate::models::ButtonActionType>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            last_midi_port: None,
            last_controller_address: Some("127.0.0.1:7348".to_string()),
            last_controller_password: None,
            last_action_type: Some(crate::models::ButtonActionType::Toggle),
        }
    }
}

pub fn check_conflicts(presets: &[Preset], new_trigger: &crate::models::MidiTrigger) -> bool {
    presets
        .iter()
        .any(|p| p.triggers.iter().any(|t| t == new_trigger))
}