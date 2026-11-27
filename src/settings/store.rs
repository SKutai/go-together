//! The settings persistence backend and the two calls that use it.
//!
//! This is the only part of `settings` that touches storage, and it touches it
//! through [`crate::platform::SettingsStorage`] rather than a filesystem or a
//! browser API: [`SettingsStore`] holds whichever adapter
//! [`crate::platform::settings_storage`] selected, and a test substitutes its
//! own.
//!
//! [`load_user_controls`] and [`save_user_controls`] are deliberately
//! asymmetric. Loading never fails, because there is always a playable
//! fallback; saving returns a [`SettingsStorageError`], because only the
//! caller knows what to tell the player. See `docs/ARCHITECTURE.md`'s
//! "Configuration ownership" for the apply-then-persist policy that caller
//! follows.

use bevy::prelude::Resource;

use crate::platform::{SettingsStorage, SettingsStorageError, settings_storage};

use super::config::ControlsConfig;

/// Selected settings persistence backend.
#[derive(Resource)]
pub(crate) struct SettingsStore {
    storage: Box<dyn SettingsStorage>,
}

impl SettingsStore {
    pub(crate) fn user() -> Self {
        Self {
            storage: settings_storage(),
        }
    }

    #[cfg(test)]
    pub(crate) fn from_storage(storage: impl SettingsStorage + 'static) -> Self {
        Self {
            storage: Box::new(storage),
        }
    }
}

/// Loads controls, preferences and match rules (schema v6), transparently
/// migrating schema v1-v5 files. Falls back to full defaults if the file
/// is missing or wholly unreadable; every valid section survives corruption
/// in another, the keyboard section included.
///
/// The returned warning is the technical detail - for a storage failure it
/// names the settings file's path and the OS error. This does not log it
/// itself, deliberately: its only production caller is [`crate::app::run`],
/// which runs before `App::new()` and therefore before Bevy's `LogPlugin`
/// installs a `tracing` subscriber, so a `warn!` here would be dispatched to
/// `NoSubscriber` and dropped. `run` gives the string two destinations that
/// do survive - the Settings screen's inline error text, and
/// [`crate::app::startup_log::DeferredStartupWarnings`], which logs it from a
/// `Startup` system once a subscriber exists.
pub(crate) fn load_user_controls(store: &SettingsStore) -> (ControlsConfig, Option<String>) {
    match store.storage.load() {
        Ok(None) => (ControlsConfig::default(), None),
        Ok(Some(json)) => match ControlsConfig::from_json(&json) {
            Ok((config, warning)) => (config, warning),
            Err(error) => (
                ControlsConfig::default(),
                Some(format!("Could not load settings: {error}. Using defaults.")),
            ),
        },
        Err(error) => (
            ControlsConfig::default(),
            Some(format!("Could not load settings: {error}. Using defaults.")),
        ),
    }
}

pub(crate) fn save_user_controls(
    store: &SettingsStore,
    config: &ControlsConfig,
) -> Result<(), SettingsStorageError> {
    let json = config.to_json().map_err(|error| {
        SettingsStorageError::Failed(format!("Could not serialize settings: {error}"))
    })?;
    store.storage.save(&json)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    use crate::match_settings::MatchSettingField;

    #[cfg(not(target_arch = "wasm32"))]
    use crate::platform::{CONFIG_FILE_NAME, NativeSettingsStorage};

    #[derive(Clone, Default)]
    struct MockSettingsStorage {
        value: Arc<Mutex<Option<String>>>,
        fail_save: bool,
        fail_load: bool,
    }

    impl SettingsStorage for MockSettingsStorage {
        fn load(&self) -> Result<Option<String>, SettingsStorageError> {
            if self.fail_load {
                return Err(SettingsStorageError::Unavailable(
                    "mock storage unavailable".into(),
                ));
            }
            Ok(self.value.lock().unwrap().clone())
        }

        fn save(&self, json: &str) -> Result<(), SettingsStorageError> {
            if self.fail_save {
                return Err(SettingsStorageError::Failed("mock quota exceeded".into()));
            }
            *self.value.lock().unwrap() = Some(json.into());
            Ok(())
        }
    }

    #[test]
    fn settings_storage_adapter_round_trips_and_surfaces_quota_failure() {
        let shared = Arc::new(Mutex::new(None));
        let storage = MockSettingsStorage {
            value: Arc::clone(&shared),
            fail_save: false,
            fail_load: false,
        };
        let store = SettingsStore::from_storage(storage);
        let mut config = ControlsConfig::default();
        config.preferences.show_match_timer = true;
        config
            .match_settings
            .set_enabled(MatchSettingField::PointTarget, true);
        config
            .match_settings
            .set_value(MatchSettingField::PointTarget, 25);

        save_user_controls(&store, &config).unwrap();
        let (loaded, warning) = load_user_controls(&store);
        assert_eq!(loaded, config);
        assert!(warning.is_none());

        let failing = SettingsStore::from_storage(MockSettingsStorage {
            value: Arc::clone(&shared),
            fail_save: true,
            fail_load: false,
        });
        let before = shared.lock().unwrap().clone();
        assert_eq!(
            save_user_controls(&failing, &ControlsConfig::default())
                .unwrap_err()
                .to_string(),
            "mock quota exceeded"
        );
        assert_eq!(*shared.lock().unwrap(), before);
    }

    /// A storage backend that cannot even be read from - not just one that
    /// rejects a write - must still produce a fully playable default
    /// configuration plus a warning, never an error the caller has to
    /// handle.
    #[test]
    fn a_storage_backend_that_cannot_be_read_falls_back_to_defaults_with_a_warning() {
        let store = SettingsStore::from_storage(MockSettingsStorage {
            value: Arc::new(Mutex::new(None)),
            fail_save: false,
            fail_load: true,
        });

        let (config, warning) = load_user_controls(&store);

        assert_eq!(config, ControlsConfig::default());
        let warning = warning.expect("an unreadable backend must produce a warning");
        assert!(warning.contains("mock storage unavailable"));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn controls_config_save_and_load_round_trip_is_atomic() {
        use crate::input::GameAction;
        use crate::input::controller::{ControllerInput, ProfileAction};
        use crate::input::keyboard::KeyboardKey;
        use crate::rules::Player;

        let root = std::env::temp_dir().join(format!(
            "go-together-controls-config-test-{}",
            uuid::Uuid::new_v4()
        ));
        let path = root.join(CONFIG_FILE_NAME);
        let storage = NativeSettingsStorage::new(path);
        let mut config = ControlsConfig::default();
        config
            .keyboard
            .assign(GameAction::RestartMatch, KeyboardKey::Enter);
        config.controller.assign(
            Player::One,
            ProfileAction::Restart,
            ControllerInput::LeftBumper,
        );
        config.preferences.show_match_timer = true;
        config.preferences.show_round_number = false;
        config
            .match_settings
            .set_enabled(MatchSettingField::MatchTime, true);
        config
            .match_settings
            .set_value(MatchSettingField::MatchTime, 3_600);

        storage.save(&config.to_json().unwrap()).unwrap();
        let json = storage.load().unwrap().unwrap();
        let (loaded, warning) = ControlsConfig::from_json(&json).unwrap();
        assert!(warning.is_none());
        assert_eq!(loaded, config);
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);

        std::fs::remove_dir_all(root).unwrap();
    }
}
