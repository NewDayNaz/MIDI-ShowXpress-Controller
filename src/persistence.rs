use anyhow::Result;
use directories::ProjectDirs;
use std::fs;
use std::path::PathBuf;

use crate::models::Preset;

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
        let presets: Vec<Preset> = serde_json::from_str(&data)?;
        Ok(presets)
    }

    pub fn save(&self, presets: &[Preset]) -> Result<()> {
        let data = serde_json::to_string_pretty(presets)?;
        fs::write(&self.file_path, data)?;
        Ok(())
    }

    pub fn load_config(&self) -> Result<AppConfig> {
        if !self.config_path.exists() {
            return Ok(AppConfig::default());
        }

        let data = fs::read_to_string(&self.config_path)?;
        let config: AppConfig = serde_json::from_str(&data)?;
        Ok(config)
    }

    pub fn save_config(&self, config: &AppConfig) -> Result<()> {
        let data = serde_json::to_string_pretty(config)?;
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