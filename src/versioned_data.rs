use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::models::Preset;
use crate::persistence::AppConfig;
use crate::versioning::{load_and_migrate_with_fallback, MigrationResult, Migration, CURRENT_VERSION};

// ============================================================================
// Versioned Presets
// ============================================================================

/// Versioned wrapper for presets list
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionedPresets {
    pub version: u32,
    pub presets: Vec<Preset>,
}

impl VersionedPresets {
    pub fn new(presets: Vec<Preset>) -> Self {
        Self {
            version: CURRENT_VERSION,
            presets,
        }
    }
}

// ============================================================================
// Preset Migrations
// ============================================================================

struct PresetMigrationV0ToV1;

impl Migration for PresetMigrationV0ToV1 {
    fn migrate(&self, from_version: u32, data: Value) -> Result<Value> {
        match from_version {
            0 => {
                // Version 0: Unversioned presets array - just wrap it
                // The data is already a valid presets array, we just need to wrap it
                Ok(json!({
                    "version": 1,
                    "presets": data
                }))
            }
            _ => Err(anyhow::anyhow!("Unknown source version for preset migration: {}", from_version)),
        }
    }
    
    fn target_version(&self) -> u32 {
        1
    }
}

// ============================================================================
// Versioned AppConfig
// ============================================================================

/// Versioned wrapper for app config
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionedAppConfig {
    pub version: u32,
    #[serde(flatten)]
    pub config: AppConfig,
}

impl VersionedAppConfig {
    pub fn new(config: AppConfig) -> Self {
        Self {
            version: CURRENT_VERSION,
            config,
        }
    }
}

// ============================================================================
// Config Migrations
// ============================================================================

struct ConfigMigrationV0ToV1;

impl Migration for ConfigMigrationV0ToV1 {
    fn migrate(&self, from_version: u32, data: Value) -> Result<Value> {
        match from_version {
            0 => {
                // Version 0: Unversioned config - deserialize it, then create versioned wrapper
                let config: AppConfig = serde_json::from_value(data.clone())
                    .map_err(|e| anyhow::anyhow!("Failed to deserialize config during migration: {}", e))?;
                
                // Create the versioned structure - since we use #[serde(flatten)], 
                // we need to merge the version with the config fields
                let mut result = serde_json::to_value(&config)?;
                if let Some(obj) = result.as_object_mut() {
                    obj.insert("version".to_string(), json!(1));
                }
                Ok(result)
            }
            _ => Err(anyhow::anyhow!("Unknown source version for config migration: {}", from_version)),
        }
    }
    
    fn target_version(&self) -> u32 {
        1
    }
}

// ============================================================================
// Migration Helpers
// ============================================================================

/// Get the list of preset migrations
fn get_preset_migrations() -> Vec<Box<dyn Migration>> {
    vec![
        Box::new(PresetMigrationV0ToV1),
    ]
}

/// Get the list of config migrations
fn get_config_migrations() -> Vec<Box<dyn Migration>> {
    vec![
        Box::new(ConfigMigrationV0ToV1),
    ]
}

/// Load and migrate presets from JSON string
pub fn load_presets(json_str: &str) -> Result<(Vec<Preset>, Option<u32>)> {
    let migrations = get_preset_migrations();
    match load_and_migrate_with_fallback::<VersionedPresets>(json_str, &migrations)? {
        MigrationResult::Current(data) => Ok((data.presets, None)),
        MigrationResult::Migrated(data, from_version) => Ok((data.presets, Some(from_version))),
    }
}

/// Load and migrate config from JSON string
pub fn load_config(json_str: &str) -> Result<(AppConfig, Option<u32>)> {
    let migrations = get_config_migrations();
    match load_and_migrate_with_fallback::<VersionedAppConfig>(json_str, &migrations)? {
        MigrationResult::Current(data) => Ok((data.config, None)),
        MigrationResult::Migrated(data, from_version) => Ok((data.config, Some(from_version))),
    }
}

/// Save presets as versioned JSON string
pub fn save_presets(presets: &[Preset]) -> Result<String> {
    let versioned = VersionedPresets::new(presets.to_vec());
    serde_json::to_string_pretty(&versioned)
        .map_err(|e| anyhow::anyhow!("Failed to serialize presets: {}", e))
}

/// Save config as versioned JSON string
pub fn save_config(config: &AppConfig) -> Result<String> {
    let versioned = VersionedAppConfig::new(config.clone());
    serde_json::to_string_pretty(&versioned)
        .map_err(|e| anyhow::anyhow!("Failed to serialize config: {}", e))
}

