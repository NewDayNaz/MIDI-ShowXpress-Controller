use anyhow::Result;
use serde::{Deserialize, Serialize};

/// Current version of the data format
pub const CURRENT_VERSION: u32 = 1;

/// Trait for migration functions
pub trait Migration: Send + Sync {
    /// Migrate data from one version to the next
    /// Takes the old version number and the JSON data, returns the migrated JSON data
    fn migrate(&self, from_version: u32, data: serde_json::Value) -> Result<serde_json::Value>;
    
    /// The version this migration migrates TO (from_version + 1)
    fn target_version(&self) -> u32;
}

/// Versioned wrapper for serialization/deserialization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionedData<T> {
    pub version: u32,
    #[serde(flatten)]
    pub data: T,
}

impl<T> VersionedData<T> {
    pub fn new(version: u32, data: T) -> Self {
        Self { version, data }
    }
    
    pub fn current(data: T) -> Self {
        Self {
            version: CURRENT_VERSION,
            data,
        }
    }
}

/// Migration result
pub enum MigrationResult<T> {
    /// Data is already at current version
    Current(T),
    /// Data was migrated from an older version
    Migrated(T, u32),
}

/// Migrate data to the current version using a list of migrations
pub fn migrate_to_current<T: for<'de> Deserialize<'de>>(
    versioned: VersionedData<serde_json::Value>,
    migrations: &[Box<dyn Migration>],
) -> Result<MigrationResult<T>> {
    let current_version = versioned.version;
    
    if current_version == CURRENT_VERSION {
        // Already at current version, reconstruct and deserialize directly
        // Since VersionedData uses flatten, we need to merge version back in
        let mut full_data = versioned.data;
        if let Some(obj) = full_data.as_object_mut() {
            obj.insert("version".to_string(), serde_json::Value::Number(current_version.into()));
        }
        let data: T = serde_json::from_value(full_data)?;
        return Ok(MigrationResult::Current(data));
    }
    
    if current_version > CURRENT_VERSION {
        return Err(anyhow::anyhow!(
            "Data version {} is newer than current version {}. Please update the application.",
            current_version,
            CURRENT_VERSION
        ));
    }
    
    // Need to migrate - start with the data (migration will return proper structure)
    let mut data = versioned.data;
    let mut version = current_version;
    
    // Apply migrations step by step
    while version < CURRENT_VERSION {
        let next_version = version + 1;
        
        // Find the migration that migrates from current version to next
        let migration = migrations.iter()
            .find(|m| m.target_version() == next_version)
            .ok_or_else(|| anyhow::anyhow!(
                "No migration found from version {} to version {}",
                version,
                next_version
            ))?;
        
        data = migration.migrate(version, data)?;
        version = next_version;
    }
    
    let final_data: T = serde_json::from_value(data)?;
    Ok(MigrationResult::Migrated(final_data, current_version))
}

/// Helper to load and migrate versioned data
pub fn load_and_migrate<T: for<'de> Deserialize<'de>>(
    json_str: &str,
    migrations: &[Box<dyn Migration>],
) -> Result<MigrationResult<T>> {
    // First, try to parse as versioned data
    let versioned: VersionedData<serde_json::Value> = serde_json::from_str(json_str)?;
    migrate_to_current(versioned, migrations)
}

/// Helper to load and migrate versioned data, with fallback for unversioned data
pub fn load_and_migrate_with_fallback<T: for<'de> Deserialize<'de>>(
    json_str: &str,
    migrations: &[Box<dyn Migration>],
) -> Result<MigrationResult<T>> {
    // Try to parse as versioned data first
    match serde_json::from_str::<VersionedData<serde_json::Value>>(json_str) {
        Ok(versioned) => migrate_to_current(versioned, migrations),
        Err(_) => {
            // If that fails, try parsing as unversioned data (version 0)
            let data: serde_json::Value = serde_json::from_str(json_str)?;
            let versioned = VersionedData::new(0, data);
            migrate_to_current(versioned, migrations)
        }
    }
}

