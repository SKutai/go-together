//! Loading every supported settings schema, and writing the current one.
//!
//! [`ControlsConfig::from_json`] and [`ControlsConfig::to_json`] are the whole
//! bridge between [`super::schema`]'s on-disk shapes and [`super::config`]'s
//! live resources; they are implemented here rather than beside
//! `ControlsConfig` itself so that a compatibility question has one file to
//! open.
//!
//! # Two kinds of failure, and why the loaders are per-section
//!
//! `crate::replay::compat` is a version ladder: each schema version has its
//! own deserialization path, and a file either loads or it does not. Settings
//! looks different because it has a second axis. A settings file is a small
//! set of independent sections, and losing one of them is not the same event
//! as losing the file:
//!
//! - **Fatal** - unparseable JSON, an oversized document, or a version this
//!   build does not know. These return [`BindingConfigError`], and
//!   [`super::load_user_controls`] falls back to complete defaults, because
//!   nothing in such a document can be told apart from noise.
//! - **Recoverable** - a missing or corrupt keyboard, controller,
//!   preferences, or game section. Each of [`load_keyboard`],
//!   [`load_controller`], [`load_preferences`], and [`load_match_settings`]
//!   returns `(value, Option<warning>)`, substituting that section's
//!   defaults and describing what it did. The player keeps everything that
//!   was still intelligible, which is what makes a partly-corrupt settings
//!   file recoverable rather than a reset. That behavior is documented in
//!   `docs/ARCHITECTURE.md` and is tested below, per section.
//!
//! The keyboard section is all-or-nothing within itself - a half-bound
//! keyboard is not a playable game, so a section that is not a complete,
//! conflict-free set of bindings is replaced whole, never patched - but that
//! is a reason to replace the section, not to discard the controller,
//! preferences and game sections beside it. It used to be fatal, and became
//! recoverable when F1 was reserved for the Help modal and a file binding
//! it had to stop loading: such a file, like one naming `escape` and, since
//! F11 became the fullscreen key, one naming `f11`, now costs its keyboard
//! bindings and nothing else. For this section "corrupt" covers a missing
//! or `null` field, a value that is not an array of `{action, key}`
//! records, a record with an unknown field, an unknown key name (which is
//! how a reserved key is refused, since none has one), an action bound twice or
//! not at all, and a key bound twice. Every one of them is a warning naming the
//! section and the cause - without serde's list of every accepted name, which
//! for a keyboard key runs longer than the Settings screen's error line.
//!
//! [`load_preferences`] applies the same idea one level down: a single bad
//! *field* costs only that field, because the preferences object is read key
//! by key through [`load_preference_field`] rather than deserialized whole.
//!
//! So the version dispatch in `from_json` decides only *which sections a
//! document has* - v1 keyboard-only under `bindings`, v2 renaming that field
//! `keyboard` and adding controller, v3 adding preferences, v4 widening
//! them, v5 adding match rules, v6 widening the preferences again - and
//! hands each section to the one loader that has always known how to read
//! it. A version reads only its own
//! keyboard field: a v1 document's `keyboard` and a later document's
//! `bindings` are never parsed, so a malformed one is ignored without a
//! warning, the way an unknown key inside `preferences` is. A new version
//! adds an arm there and, at most, one loader; it does not fork the ones
//! that already exist.
//!
//! [`BindingConfigError`]: super::schema::BindingConfigError

use serde::Deserialize;

use crate::input::controller::ControllerBindings;
use crate::input::keyboard::KeyboardBindings;
use crate::match_settings::MatchSettings;
use crate::rules::Player;

use super::config::{ControlsConfig, GamePreferences};
use super::schema::{
    BindingConfigError, BindingRecord, CONFIG_VERSION, CONTROLS_SCHEMA_VERSION,
    ControllerBindingRecord, ControllerProfilesRecord, ControlsFile, ControlsLoadFile,
    LEGACY_SETTINGS_SCHEMA_VERSION, MATCH_RULES_SETTINGS_SCHEMA_VERSION, MAX_SETTINGS_JSON_BYTES,
    SETTINGS_SCHEMA_VERSION, SPLIT_AUDIO_SETTINGS_SCHEMA_VERSION,
};

impl ControlsConfig {
    /// Reads a settings document of any supported schema version, migrating
    /// it onto the current shape.
    ///
    /// The `Ok` warning is the recoverable-failure channel described in this
    /// module's docs: `Some` means at least one section fell back to its
    /// defaults, and the string says which. An `Err` means nothing in the
    /// document could be used.
    pub(crate) fn from_json(json: &str) -> Result<(Self, Option<String>), BindingConfigError> {
        if json.len() > MAX_SETTINGS_JSON_BYTES {
            return Err(BindingConfigError::DataTooLarge);
        }
        let file: ControlsLoadFile =
            serde_json::from_str(json).map_err(BindingConfigError::InvalidJson)?;
        match file.version {
            CONFIG_VERSION => {
                // Legacy schema v1: keyboard-only file. Migrate by keeping
                // the custom keyboard bindings and supplying controller
                // defaults; this is a normal, silent upgrade path.
                let (keyboard, keyboard_warning) = load_keyboard(file.bindings);
                Ok((
                    Self {
                        keyboard,
                        controller: ControllerBindings::default(),
                        preferences: GamePreferences::default(),
                        match_settings: MatchSettings::default(),
                    },
                    keyboard_warning,
                ))
            }
            CONTROLS_SCHEMA_VERSION => {
                let (keyboard, keyboard_warning) = load_keyboard(file.keyboard);
                let (controller, controller_warning) = load_controller(file.controller);
                Ok((
                    Self {
                        keyboard,
                        controller,
                        preferences: GamePreferences::default(),
                        match_settings: MatchSettings::default(),
                    },
                    join_warnings([keyboard_warning, controller_warning]),
                ))
            }
            LEGACY_SETTINGS_SCHEMA_VERSION
            | SPLIT_AUDIO_SETTINGS_SCHEMA_VERSION
            | MATCH_RULES_SETTINGS_SCHEMA_VERSION
            | SETTINGS_SCHEMA_VERSION => {
                let (keyboard, keyboard_warning) = load_keyboard(file.keyboard);
                let (controller, controller_warning) = load_controller(file.controller);
                let (preferences, preference_warning) =
                    load_preferences(file.preferences, file.version);
                // Gated on the version that introduced the section, never
                // on the current one: the next version bump must not cost
                // every existing file its Match rules.
                let (match_settings, game_warning) =
                    if file.version >= MATCH_RULES_SETTINGS_SCHEMA_VERSION {
                        load_match_settings(file.game)
                    } else {
                        (MatchSettings::default(), None)
                    };
                Ok((
                    Self {
                        keyboard,
                        controller,
                        preferences,
                        match_settings,
                    },
                    join_warnings([
                        keyboard_warning,
                        controller_warning,
                        preference_warning,
                        game_warning,
                    ]),
                ))
            }
            other => Err(BindingConfigError::UnsupportedVersion(other)),
        }
    }

    /// Writes the current schema version, and only the current one. There is
    /// no "save as an older version" direction: unlike a Replay, a settings
    /// document is this build's own state, not a historical record.
    pub(crate) fn to_json(&self) -> Result<String, BindingConfigError> {
        serde_json::to_string_pretty(&ControlsFile::from(self))
            .map_err(BindingConfigError::InvalidJson)
    }
}

impl From<&ControlsConfig> for ControlsFile {
    fn from(config: &ControlsConfig) -> Self {
        Self {
            version: SETTINGS_SCHEMA_VERSION,
            bindings: Vec::new(),
            keyboard: config
                .keyboard
                .iter()
                .map(|(action, key)| BindingRecord { action, key })
                .collect(),
            controller: Some(ControllerProfilesRecord {
                player_one: config
                    .controller
                    .iter_profile(Player::One)
                    .map(|(action, input)| ControllerBindingRecord { action, input })
                    .collect(),
                player_two: config
                    .controller
                    .iter_profile(Player::Two)
                    .map(|(action, input)| ControllerBindingRecord { action, input })
                    .collect(),
            }),
            preferences: Some(config.preferences),
            game: Some(config.match_settings.clone()),
        }
    }
}

/// The per-section warnings of one document as one string, in section
/// order, or `None` when every section loaded cleanly.
fn join_warnings<const N: usize>(warnings: [Option<String>; N]) -> Option<String> {
    let warning = warnings.into_iter().flatten().collect::<Vec<_>>().join(" ");
    (!warning.is_empty()).then_some(warning)
}

/// Reads a schema's keyboard section, falling back to the default bindings
/// with a warning if it is missing, unreadable, or not a complete,
/// conflict-free set of bindings. Shared by every schema version, since the
/// `{action, key}` record shape has never changed - only the field it lives
/// under has. The section is kept whole or replaced whole; the module docs
/// say why, and list every shape that counts as unreadable.
fn load_keyboard(value: Option<serde_json::Value>) -> (KeyboardBindings, Option<String>) {
    let Some(value) = value else {
        return (
            KeyboardBindings::default(),
            Some("Keyboard bindings were missing; using keyboard defaults.".into()),
        );
    };
    let records: Vec<BindingRecord> = match serde_json::from_value(value) {
        Ok(records) => records,
        Err(error) => {
            return (
                KeyboardBindings::default(),
                Some(format!(
                    "Keyboard bindings were invalid ({}); using keyboard defaults.",
                    without_expected_names(&error)
                )),
            );
        }
    };
    match KeyboardBindings::try_from_entries(
        records
            .into_iter()
            .map(|record| (record.action, record.key))
            .collect(),
    ) {
        Ok(keyboard) => (keyboard, None),
        Err(error) => (
            KeyboardBindings::default(),
            Some(format!(
                "Keyboard bindings were invalid ({error}); using keyboard defaults."
            )),
        ),
    }
}

/// Serde's message for an unknown enum name lists every accepted name after
/// "expected one of" - ninety-odd for a keyboard key, twenty for a
/// controller input - which is longer than the Settings screen's error line
/// and tells the player nothing the part before it does not: the name that
/// was not accepted. Cut at the *last* marker: the name is the file's own
/// text and may contain the marker itself, while the list after the real
/// one is nothing but backticked identifiers and never can.
fn without_expected_names(error: &serde_json::Error) -> String {
    let message = error.to_string();
    match message.rsplit_once(", expected one of ") {
        Some((cause, _)) => cause.to_string(),
        None => message,
    }
}

/// Reads the controller section added by schema v2, falling back to default
/// profiles with a warning if it is missing, unreadable, or not a valid pair
/// of profiles.
fn load_controller(value: Option<serde_json::Value>) -> (ControllerBindings, Option<String>) {
    let Some(value) = value else {
        return (
            ControllerBindings::default(),
            Some("Controller bindings were missing; using controller defaults.".into()),
        );
    };
    let record: ControllerProfilesRecord = match serde_json::from_value(value) {
        Ok(record) => record,
        Err(error) => {
            return (
                ControllerBindings::default(),
                Some(format!(
                    "Controller bindings were invalid ({}); using controller defaults.",
                    without_expected_names(&error)
                )),
            );
        }
    };
    let player_one = record
        .player_one
        .into_iter()
        .map(|entry| (entry.action, entry.input))
        .collect();
    let player_two = record
        .player_two
        .into_iter()
        .map(|entry| (entry.action, entry.input))
        .collect();
    match ControllerBindings::try_from_entries(player_one, player_two) {
        Ok(controller) => (controller, None),
        Err(error) => (
            ControllerBindings::default(),
            Some(format!(
                "Controller bindings were invalid ({error}); using controller defaults."
            )),
        ),
    }
}

/// Reads the preferences section added by schema v3, field by field.
///
/// `schema_version` is what keeps an older file silent: v3 had no sound
/// levels and nothing before v6 had Reduce Motion, so their absence is an
/// upgrade rather than corruption, and asking for them would produce a
/// warning on every load of an older file.
///
/// Reading key by key has one side effect worth knowing: a key inside
/// `preferences` that this build does not recognize is ignored without a
/// warning, because nothing here ever inspects a key it did not ask for.
/// [`GamePreferences`]'s own `deny_unknown_fields` bites only when a test
/// deserializes a whole `ControlsFile`. That is looser than the envelope,
/// where an unknown top-level field is fatal, and than the controller and
/// game sections, where an unknown field costs that section its stored values
/// and produces a warning.
fn load_preferences(
    value: Option<serde_json::Value>,
    schema_version: u32,
) -> (GamePreferences, Option<String>) {
    let Some(value) = value else {
        return (
            GamePreferences::default(),
            Some("Preferences were missing; using preference defaults.".into()),
        );
    };
    let Some(object) = value.as_object() else {
        return (
            GamePreferences::default(),
            Some("Preferences were invalid; using preference defaults.".into()),
        );
    };
    let mut preferences = GamePreferences::default();
    let mut warnings = Vec::new();
    load_preference_field(
        object,
        "show_match_timer",
        &mut preferences.show_match_timer,
        &mut warnings,
    );
    load_preference_field(
        object,
        "show_round_number",
        &mut preferences.show_round_number,
        &mut warnings,
    );
    if schema_version >= SPLIT_AUDIO_SETTINGS_SCHEMA_VERSION {
        load_preference_field(
            object,
            "game_sound_effects",
            &mut preferences.game_sound_effects,
            &mut warnings,
        );
        load_preference_field(
            object,
            "ui_sound_effects",
            &mut preferences.ui_sound_effects,
            &mut warnings,
        );
    }
    if schema_version >= SETTINGS_SCHEMA_VERSION {
        load_preference_field(
            object,
            "reduce_motion",
            &mut preferences.reduce_motion,
            &mut warnings,
        );
    }
    (
        preferences,
        (!warnings.is_empty()).then(|| warnings.join(" ")),
    )
}

/// Reads the match-rules section added by schema v5, falling back to the
/// default preset with a warning. Only called for v5 and later; see
/// [`ControlsConfig::from_json`].
fn load_match_settings(value: Option<serde_json::Value>) -> (MatchSettings, Option<String>) {
    let Some(value) = value else {
        return (
            MatchSettings::default(),
            Some("Game settings were missing; using game defaults.".into()),
        );
    };
    match serde_json::from_value(value) {
        Ok(settings) => (settings, None),
        Err(error) => (
            MatchSettings::default(),
            Some(format!(
                "Game settings were invalid ({error}); using game defaults."
            )),
        ),
    }
}

/// Reads one preference, leaving `target` at its default and appending a
/// warning if the key is absent or does not deserialize. This per-field
/// recovery is why [`load_preferences`] does not simply deserialize a
/// [`GamePreferences`].
fn load_preference_field<T>(
    object: &serde_json::Map<String, serde_json::Value>,
    field: &str,
    target: &mut T,
    warnings: &mut Vec<String>,
) where
    T: for<'de> Deserialize<'de>,
{
    let Some(value) = object.get(field) else {
        warnings.push(format!(
            "Preference {field} was missing; using its default."
        ));
        return;
    };
    match serde_json::from_value(value.clone()) {
        Ok(loaded) => *target = loaded,
        Err(error) => warnings.push(format!(
            "Preference {field} was invalid ({error}); using its default."
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::input::GameAction;
    use crate::input::controller::{ControllerInput, ProfileAction};
    use crate::input::keyboard::KeyboardKey;
    use crate::match_settings::MatchSettingField;
    use crate::settings::VolumeLevel;

    fn legacy_controls_file(bindings: &KeyboardBindings) -> ControlsFile {
        ControlsFile {
            version: CONFIG_VERSION,
            bindings: bindings
                .iter()
                .map(|(action, key)| BindingRecord { action, key })
                .collect(),
            keyboard: Vec::new(),
            controller: None,
            preferences: None,
            game: None,
        }
    }

    fn historical_wasd_white_bindings() -> KeyboardBindings {
        KeyboardBindings::try_from_entries(
            KeyboardBindings::default()
                .iter()
                .map(|(action, key)| {
                    let key = match action {
                        GameAction::PlayerTwoUp => KeyboardKey::W,
                        GameAction::PlayerTwoDown => KeyboardKey::S,
                        GameAction::PlayerTwoLeft => KeyboardKey::A,
                        GameAction::PlayerTwoRight => KeyboardKey::D,
                        GameAction::PlayerTwoSelect => KeyboardKey::Backtick,
                        GameAction::PlayerTwoSetMarker => KeyboardKey::Digit1,
                        GameAction::PlayerTwoJumpToMarker => KeyboardKey::Digit2,
                        _ => key,
                    };
                    (action, key)
                })
                .collect(),
        )
        .unwrap()
    }

    /// A v1 document is its keyboard section, so an unusable one yields the
    /// defaults - as an `Ok` with a warning naming the cause, the same
    /// channel every other section uses, rather than the fatal path a
    /// document that does not parse takes.
    #[test]
    fn an_unusable_legacy_keyboard_section_falls_back_with_a_warning() {
        let load = |json: &str| {
            let (config, warning) = ControlsConfig::from_json(json).unwrap();
            assert_eq!(config, ControlsConfig::default());
            warning.unwrap()
        };

        let missing = r#"{"version":1,"bindings":[]}"#;
        assert_eq!(
            load(missing),
            "Keyboard bindings were invalid (not every gameplay action is bound); using \
             keyboard defaults."
        );

        let unknown = serde_json::to_string(&legacy_controls_file(&KeyboardBindings::default()))
            .unwrap()
            .replace("\"up_arrow\"", "\"not_a_key\"");
        assert_eq!(
            load(&unknown),
            "Keyboard bindings were invalid (unknown variant `not_a_key`); using keyboard \
             defaults."
        );

        let mut duplicate = legacy_controls_file(&KeyboardBindings::default());
        duplicate.bindings[1].key = duplicate.bindings[0].key;
        assert_eq!(
            load(&serde_json::to_string(&duplicate).unwrap()),
            "Keyboard bindings were invalid (duplicate active key Up Arrow); using keyboard \
             defaults."
        );

        duplicate = legacy_controls_file(&KeyboardBindings::default());
        duplicate.bindings[1].action = duplicate.bindings[0].action;
        assert_eq!(
            load(&serde_json::to_string(&duplicate).unwrap()),
            "Keyboard bindings were invalid (duplicate action PlayerOneUp); using keyboard \
             defaults."
        );
    }

    /// A document of any version whose keyboard section carries a key the
    /// interface reserves for itself keeps everything but that section.
    /// All three names take the same path, because none has a
    /// `KeyboardKey`: `escape` never did, `f1` stopped having one when F1
    /// became the Help key, and `f11` when F11 became the fullscreen key -
    /// the one reason a key name has ever been withdrawn from the format,
    /// taken twice. The warning carries the name and not serde's list of
    /// every accepted one. The same document naming the function keys
    /// beside them, F10 and F12, loads whole and binds that key.
    #[test]
    fn a_reserved_key_in_any_schema_version_costs_only_the_keyboard_section() {
        let mut config = ControlsConfig::default();
        config
            .keyboard
            .assign(GameAction::PlayerOneSelect, KeyboardKey::F8);
        config.controller.assign(
            Player::Two,
            ProfileAction::SetMarker,
            ControllerInput::RightBumper,
        );
        config.preferences.show_match_timer = true;
        config.preferences.reduce_motion = true;
        config
            .match_settings
            .set_enabled(MatchSettingField::RoundLimit, true);

        for version in CONFIG_VERSION..=SETTINGS_SCHEMA_VERSION {
            for (name, reserved) in [
                ("escape", true),
                ("f1", true),
                ("f11", true),
                ("f10", false),
                ("f12", false),
            ] {
                let mut file: serde_json::Value =
                    serde_json::from_str(&config.to_json().unwrap()).unwrap();
                file["version"] = version.into();
                let document = file.as_object_mut().unwrap();
                if version == CONFIG_VERSION {
                    let keyboard = document.remove("keyboard").unwrap();
                    document.insert("bindings".into(), keyboard);
                    document.remove("controller");
                }
                if version < LEGACY_SETTINGS_SCHEMA_VERSION {
                    document.remove("preferences");
                } else {
                    let preferences = document["preferences"].as_object_mut().unwrap();
                    if version == LEGACY_SETTINGS_SCHEMA_VERSION {
                        preferences.remove("game_sound_effects");
                        preferences.remove("ui_sound_effects");
                    }
                    if version < SETTINGS_SCHEMA_VERSION {
                        preferences.remove("reduce_motion");
                    }
                }
                if version < MATCH_RULES_SETTINGS_SCHEMA_VERSION {
                    document.remove("game");
                }
                let field = if version == CONFIG_VERSION {
                    "bindings"
                } else {
                    "keyboard"
                };
                assert_eq!(file[field][0]["action"], "player_one_up");
                file[field][0]["key"] = name.into();

                let (loaded, warning) = ControlsConfig::from_json(&file.to_string()).unwrap();
                if !reserved {
                    assert_eq!(warning, None, "schema v{version}, {name}");
                    let key: KeyboardKey = serde_json::from_value(name.into()).unwrap();
                    assert_eq!(loaded.keyboard.key(GameAction::PlayerOneUp), key);
                    assert_eq!(
                        loaded.keyboard.key(GameAction::PlayerOneSelect),
                        KeyboardKey::F8,
                        "v{version}"
                    );
                    continue;
                }
                assert_eq!(
                    warning.as_deref(),
                    Some(
                        format!(
                            "Keyboard bindings were invalid (unknown variant `{name}`); using \
                             keyboard defaults."
                        )
                        .as_str()
                    ),
                    "schema v{version}, {name}"
                );
                assert_eq!(loaded.keyboard, KeyboardBindings::default(), "v{version}");
                if version >= CONTROLS_SCHEMA_VERSION {
                    assert_eq!(loaded.controller, config.controller, "v{version}");
                }
                if version >= LEGACY_SETTINGS_SCHEMA_VERSION {
                    assert!(loaded.preferences.show_match_timer, "v{version}");
                }
                if version >= MATCH_RULES_SETTINGS_SCHEMA_VERSION {
                    assert_eq!(loaded.match_settings, config.match_settings);
                }
                assert_eq!(
                    loaded.preferences.reduce_motion,
                    version >= SETTINGS_SCHEMA_VERSION,
                    "v{version}"
                );
            }
        }
    }

    /// Every shape of keyboard section the loader cannot read is one
    /// warning and one section's defaults, with the controller profiles
    /// beside it untouched: absent, `null`, not an array, an empty array,
    /// and a record with a field this build does not know. An unknown
    /// *top-level* field is still fatal (`unknown_top_level_settings_fields_remain_rejected`).
    #[test]
    fn every_unreadable_keyboard_section_shape_falls_back_alone() {
        let mut config = ControlsConfig::default();
        config.controller.assign(
            Player::Two,
            ProfileAction::SetMarker,
            ControllerInput::RightBumper,
        );
        let base: serde_json::Value = serde_json::from_str(&config.to_json().unwrap()).unwrap();

        let cases: [(&str, Option<serde_json::Value>, &str); 5] = [
            (
                "absent",
                None,
                "Keyboard bindings were missing; using keyboard defaults.",
            ),
            (
                "null",
                Some(serde_json::Value::Null),
                "Keyboard bindings were missing; using keyboard defaults.",
            ),
            (
                "not an array",
                Some(serde_json::json!({})),
                "Keyboard bindings were invalid (invalid type: map, expected a sequence); using \
                 keyboard defaults.",
            ),
            (
                "empty",
                Some(serde_json::json!([])),
                "Keyboard bindings were invalid (not every gameplay action is bound); using \
                 keyboard defaults.",
            ),
            (
                "unknown record field",
                Some({
                    let mut records = base["keyboard"].clone();
                    records[0]["repeat"] = true.into();
                    records
                }),
                "Keyboard bindings were invalid (unknown field `repeat`, expected `action` or \
                 `key`); using keyboard defaults.",
            ),
        ];
        for (case, keyboard, expected) in cases {
            let mut file = base.clone();
            match keyboard {
                Some(value) => file["keyboard"] = value,
                None => {
                    file.as_object_mut().unwrap().remove("keyboard");
                }
            }
            let (loaded, warning) = ControlsConfig::from_json(&file.to_string()).unwrap();
            assert_eq!(warning.as_deref(), Some(expected), "{case}");
            assert_eq!(loaded.keyboard, KeyboardBindings::default(), "{case}");
            assert_eq!(loaded.controller, config.controller, "{case}");
        }
    }

    /// The name in a warning is the file's own text, so a name built to look
    /// like serde's own marker must still come through whole - the cut is at
    /// the last marker, and the list after it can never contain one.
    #[test]
    fn a_key_name_containing_serdes_own_marker_is_kept_whole() {
        let config = ControlsConfig::default();
        let mut file: serde_json::Value = serde_json::from_str(&config.to_json().unwrap()).unwrap();
        file["keyboard"][0]["key"] = "a, expected one of b".into();
        let (loaded, warning) = ControlsConfig::from_json(&file.to_string()).unwrap();
        assert_eq!(loaded.keyboard, KeyboardBindings::default());
        assert_eq!(
            warning.as_deref(),
            Some(
                "Keyboard bindings were invalid (unknown variant `a, expected one of b`); using \
                 keyboard defaults."
            )
        );
    }

    /// A version reads only its own keyboard field. The other, if a file
    /// carries it, is never parsed - so a malformed one is ignored without a
    /// warning, which is the same looseness `load_preferences` documents for
    /// an unknown key inside `preferences`.
    #[test]
    fn a_keyboard_field_the_version_does_not_read_is_not_parsed() {
        let mut config = ControlsConfig::default();
        config
            .keyboard
            .assign(GameAction::PlayerOneSelect, KeyboardKey::F8);

        let mut current: serde_json::Value =
            serde_json::from_str(&config.to_json().unwrap()).unwrap();
        current["bindings"] = "not even an array".into();
        let (loaded, warning) = ControlsConfig::from_json(&current.to_string()).unwrap();
        assert!(warning.is_none());
        assert_eq!(loaded.keyboard, config.keyboard);

        let mut legacy: serde_json::Value =
            serde_json::to_value(legacy_controls_file(&config.keyboard)).unwrap();
        legacy["keyboard"] = "not even an array".into();
        let (loaded, warning) = ControlsConfig::from_json(&legacy.to_string()).unwrap();
        assert!(warning.is_none());
        assert_eq!(loaded.keyboard, config.keyboard);
    }

    /// A v2 document reports both of its sections, in order, when both are
    /// unusable - the v3+ arm always joined its warnings, and the v2 arm now
    /// has two to join.
    #[test]
    fn a_v2_document_joins_its_keyboard_and_controller_warnings() {
        let config = ControlsConfig::default();
        let mut file: serde_json::Value = serde_json::from_str(&config.to_json().unwrap()).unwrap();
        file["version"] = CONTROLS_SCHEMA_VERSION.into();
        file.as_object_mut().unwrap().remove("preferences");
        file.as_object_mut().unwrap().remove("game");
        file["keyboard"] = serde_json::json!([]);
        file.as_object_mut().unwrap().remove("controller");

        let (loaded, warning) = ControlsConfig::from_json(&file.to_string()).unwrap();
        assert_eq!(loaded, ControlsConfig::default());
        assert_eq!(
            warning.as_deref(),
            Some(
                "Keyboard bindings were invalid (not every gameplay action is bound); using \
                 keyboard defaults. Controller bindings were missing; using controller defaults."
            )
        );
    }

    #[test]
    fn schema_v6_round_trips_controls_preferences_and_match_settings() {
        let mut config = ControlsConfig::default();
        config
            .keyboard
            .assign(GameAction::PlayerOneSelect, KeyboardKey::F8);
        config.controller.assign(
            Player::Two,
            ProfileAction::SetMarker,
            ControllerInput::RightBumper,
        );
        config.preferences.show_match_timer = true;
        config.preferences.show_round_number = false;
        config.preferences.game_sound_effects = VolumeLevel::High;
        config.preferences.ui_sound_effects = VolumeLevel::Low;
        config.preferences.reduce_motion = true;
        config
            .match_settings
            .set_enabled(MatchSettingField::RoundLimit, true);
        config
            .match_settings
            .set_value(MatchSettingField::RoundLimit, 42);

        let json = config.to_json().unwrap();
        assert!(json.contains("\"version\": 6"));
        assert!(json.contains("\"game\""));
        assert!(json.contains("\"reduce_motion\": true"));
        let (loaded, warning) = ControlsConfig::from_json(&json).unwrap();

        assert!(warning.is_none());
        assert_eq!(loaded, config);
    }

    /// A v5 document is what every player has on disk before this build:
    /// controls, every v5 preference, and the Match rules. All of it
    /// survives, Match rules included, and the one field v5 never had is
    /// Reduce Motion, which takes its default in silence. The Match rules
    /// are the assertion worth having: the loader gates them on the version
    /// that introduced them, and gating them on the current version instead
    /// would have cost every v5 file its preset without a warning.
    #[test]
    fn schema_v5_migrates_silently_keeping_its_match_rules_with_motion_at_its_default() {
        let mut config = ControlsConfig::default();
        config
            .keyboard
            .assign(GameAction::PlayerOneSelect, KeyboardKey::F8);
        config.preferences.show_match_timer = true;
        config.preferences.ui_sound_effects = VolumeLevel::Low;
        config
            .match_settings
            .set_enabled(MatchSettingField::PointTarget, true);
        config
            .match_settings
            .set_value(MatchSettingField::PointTarget, 21);
        let mut file: serde_json::Value = serde_json::from_str(&config.to_json().unwrap()).unwrap();
        file["version"] = MATCH_RULES_SETTINGS_SCHEMA_VERSION.into();
        file["preferences"]
            .as_object_mut()
            .unwrap()
            .remove("reduce_motion");

        let (migrated, warning) = ControlsConfig::from_json(&file.to_string()).unwrap();

        assert!(warning.is_none(), "{warning:?}");
        assert_eq!(migrated.keyboard, config.keyboard);
        assert!(migrated.preferences.show_match_timer);
        assert_eq!(migrated.preferences.ui_sound_effects, VolumeLevel::Low);
        assert!(!migrated.preferences.reduce_motion);
        assert_eq!(migrated.match_settings, config.match_settings);
    }

    /// The gate's other half: a v6 document that lacks the field is not an
    /// older document, so the absence is reported like any other missing
    /// preference, and the rest of the section still loads.
    #[test]
    fn a_v6_document_missing_reduce_motion_warns_and_keeps_the_rest() {
        let mut config = ControlsConfig::default();
        config.preferences.show_match_timer = true;
        let mut file: serde_json::Value = serde_json::from_str(&config.to_json().unwrap()).unwrap();
        file["preferences"]
            .as_object_mut()
            .unwrap()
            .remove("reduce_motion");

        let (loaded, warning) = ControlsConfig::from_json(&file.to_string()).unwrap();

        assert_eq!(
            warning.as_deref(),
            Some("Preference reduce_motion was missing; using its default.")
        );
        assert!(loaded.preferences.show_match_timer);
        assert!(!loaded.preferences.reduce_motion);
    }

    #[test]
    fn schema_v3_migrates_controls_and_display_preferences_with_medium_audio() {
        let mut config = ControlsConfig::default();
        config
            .keyboard
            .assign(GameAction::PlayerOneSelect, KeyboardKey::F8);
        config.preferences.show_match_timer = true;
        config.preferences.show_round_number = false;
        let mut file: serde_json::Value = serde_json::from_str(&config.to_json().unwrap()).unwrap();
        file["version"] = LEGACY_SETTINGS_SCHEMA_VERSION.into();
        file.as_object_mut().unwrap().remove("game");
        let preferences = file["preferences"].as_object_mut().unwrap();
        preferences.remove("game_sound_effects");
        preferences.remove("ui_sound_effects");
        preferences.remove("reduce_motion");

        let (migrated, warning) = ControlsConfig::from_json(&file.to_string()).unwrap();

        assert!(warning.is_none());
        assert_eq!(migrated.keyboard, config.keyboard);
        assert!(migrated.preferences.show_match_timer);
        assert!(!migrated.preferences.show_round_number);
        assert_eq!(migrated.preferences.game_sound_effects, VolumeLevel::Medium);
        assert_eq!(migrated.preferences.ui_sound_effects, VolumeLevel::Medium);
        assert!(!migrated.preferences.reduce_motion);
        assert_eq!(migrated.match_settings, MatchSettings::default());
    }

    #[test]
    fn legacy_schema_v1_file_migrates_preserving_custom_keyboard_bindings() {
        let mut legacy_keyboard = historical_wasd_white_bindings();
        legacy_keyboard.assign(GameAction::PlayerTwoSelect, KeyboardKey::F8);
        let legacy_json =
            serde_json::to_string_pretty(&legacy_controls_file(&legacy_keyboard)).unwrap();
        assert!(legacy_json.contains("\"version\": 1"));

        let (migrated, warning) = ControlsConfig::from_json(&legacy_json).unwrap();

        assert!(warning.is_none());
        assert_eq!(migrated.keyboard, legacy_keyboard);
        assert_eq!(migrated.controller, ControllerBindings::default());
        assert_eq!(migrated.preferences, GamePreferences::default());
        assert_eq!(migrated.match_settings, MatchSettings::default());
    }

    #[test]
    fn every_stored_schema_preserves_historical_wasd_white_bindings() {
        let historical = historical_wasd_white_bindings();
        let legacy_v1 = serde_json::to_string(&legacy_controls_file(&historical)).unwrap();
        let (loaded_v1, warning) = ControlsConfig::from_json(&legacy_v1).unwrap();
        assert!(warning.is_none());
        assert_eq!(loaded_v1.keyboard, historical);

        for version in 2..=SETTINGS_SCHEMA_VERSION {
            let config = ControlsConfig {
                keyboard: historical.clone(),
                ..ControlsConfig::default()
            };
            let mut file: serde_json::Value =
                serde_json::from_str(&config.to_json().unwrap()).unwrap();
            file["version"] = version.into();
            if version == CONTROLS_SCHEMA_VERSION {
                file.as_object_mut().unwrap().remove("preferences");
                file.as_object_mut().unwrap().remove("game");
            } else if version == LEGACY_SETTINGS_SCHEMA_VERSION {
                let preferences = file["preferences"].as_object_mut().unwrap();
                preferences.remove("game_sound_effects");
                preferences.remove("ui_sound_effects");
                preferences.remove("reduce_motion");
                file.as_object_mut().unwrap().remove("game");
            } else if version == SPLIT_AUDIO_SETTINGS_SCHEMA_VERSION {
                file["preferences"]
                    .as_object_mut()
                    .unwrap()
                    .remove("reduce_motion");
                file.as_object_mut().unwrap().remove("game");
            } else if version == MATCH_RULES_SETTINGS_SCHEMA_VERSION {
                file["preferences"]
                    .as_object_mut()
                    .unwrap()
                    .remove("reduce_motion");
            }

            let (loaded, warning) = ControlsConfig::from_json(&file.to_string()).unwrap();
            assert!(
                warning.is_none(),
                "schema v{version} should migrate silently"
            );
            assert_eq!(
                loaded.keyboard, historical,
                "schema v{version} changed explicitly stored bindings"
            );
            if version < MATCH_RULES_SETTINGS_SCHEMA_VERSION {
                assert_eq!(loaded.match_settings, MatchSettings::default());
            }
        }
    }

    #[test]
    fn schema_v2_migrates_custom_controls_with_preference_defaults() {
        let mut config = ControlsConfig::default();
        config
            .keyboard
            .assign(GameAction::PlayerOneSelect, KeyboardKey::F8);
        config.controller.assign(
            Player::Two,
            ProfileAction::SetMarker,
            ControllerInput::RightBumper,
        );
        let mut file: serde_json::Value = serde_json::from_str(&config.to_json().unwrap()).unwrap();
        file["version"] = 2.into();
        file.as_object_mut().unwrap().remove("preferences");
        file.as_object_mut().unwrap().remove("game");

        let (migrated, warning) = ControlsConfig::from_json(&file.to_string()).unwrap();

        assert!(warning.is_none());
        assert_eq!(migrated.keyboard, config.keyboard);
        assert_eq!(migrated.controller, config.controller);
        assert_eq!(migrated.preferences, GamePreferences::default());
        assert_eq!(migrated.match_settings, MatchSettings::default());
    }

    #[test]
    fn invalid_controller_section_recovers_valid_keyboard_with_warning() {
        let mut config = ControlsConfig::default();
        config
            .keyboard
            .assign(GameAction::PlayerOneSelect, KeyboardKey::F8);
        let mut file: ControlsFile = serde_json::from_str(&config.to_json().unwrap()).unwrap();
        // Duplicate an action within a controller profile to make it invalid.
        let mut controller = file.controller.unwrap();
        controller.player_one[1].action = controller.player_one[0].action;
        file.controller = Some(controller);

        let (recovered, warning) =
            ControlsConfig::from_json(&serde_json::to_string(&file).unwrap()).unwrap();

        assert!(warning.is_some());
        assert_eq!(recovered.keyboard, config.keyboard);
        assert_eq!(recovered.controller, ControllerBindings::default());
        assert_eq!(recovered.preferences, config.preferences);
    }

    #[test]
    fn missing_controller_section_recovers_valid_keyboard_with_warning() {
        let config = ControlsConfig::default();
        let mut file: ControlsFile = serde_json::from_str(&config.to_json().unwrap()).unwrap();
        file.controller = None;

        let (recovered, warning) =
            ControlsConfig::from_json(&serde_json::to_string(&file).unwrap()).unwrap();

        assert!(warning.is_some());
        assert_eq!(recovered.keyboard, config.keyboard);
        assert_eq!(recovered.controller, ControllerBindings::default());
        assert_eq!(recovered.preferences, config.preferences);
    }

    #[test]
    fn corrupt_or_missing_preferences_fall_back_without_losing_controls() {
        let mut config = ControlsConfig::default();
        config
            .keyboard
            .assign(GameAction::PlayerOneSelect, KeyboardKey::F8);
        config.controller.assign(
            Player::Two,
            ProfileAction::SetMarker,
            ControllerInput::RightBumper,
        );
        let mut file: serde_json::Value = serde_json::from_str(&config.to_json().unwrap()).unwrap();
        file["preferences"] = serde_json::json!({
            "show_match_timer": "yes",
            "show_round_number": true,
            "game_sound_effects": "loudest",
            "ui_sound_effects": "low",
            "reduce_motion": "sometimes"
        });

        let (recovered, warning) = ControlsConfig::from_json(&file.to_string()).unwrap();
        let warning = warning.unwrap();
        assert!(warning.contains("show_match_timer"));
        assert!(warning.contains("game_sound_effects"));
        assert!(warning.contains("reduce_motion"));
        assert_eq!(recovered.keyboard, config.keyboard);
        assert_eq!(recovered.controller, config.controller);
        assert!(!recovered.preferences.show_match_timer);
        assert!(recovered.preferences.show_round_number);
        assert_eq!(
            recovered.preferences.game_sound_effects,
            VolumeLevel::Medium
        );
        assert_eq!(recovered.preferences.ui_sound_effects, VolumeLevel::Low);
        assert!(!recovered.preferences.reduce_motion);

        file.as_object_mut().unwrap().remove("preferences");
        let (recovered, warning) = ControlsConfig::from_json(&file.to_string()).unwrap();
        assert!(warning.unwrap().contains("Preferences were missing"));
        assert_eq!(recovered.keyboard, config.keyboard);
        assert_eq!(recovered.controller, config.controller);
        assert_eq!(recovered.preferences, GamePreferences::default());
    }

    #[test]
    fn corrupt_or_missing_game_section_falls_back_without_losing_other_settings() {
        let mut config = ControlsConfig::default();
        config
            .keyboard
            .assign(GameAction::PlayerOneSelect, KeyboardKey::F8);
        config.preferences.show_match_timer = true;
        let mut file: serde_json::Value = serde_json::from_str(&config.to_json().unwrap()).unwrap();
        file["game"]["round_limit"]["value"] = serde_json::json!("many");

        let (recovered, warning) = ControlsConfig::from_json(&file.to_string()).unwrap();
        assert!(warning.unwrap().contains("Game settings were invalid"));
        assert_eq!(recovered.keyboard, config.keyboard);
        assert_eq!(recovered.controller, config.controller);
        assert_eq!(recovered.preferences, config.preferences);
        assert_eq!(recovered.match_settings, MatchSettings::default());

        file.as_object_mut().unwrap().remove("game");
        let (recovered, warning) = ControlsConfig::from_json(&file.to_string()).unwrap();
        assert!(warning.unwrap().contains("Game settings were missing"));
        assert_eq!(recovered.keyboard, config.keyboard);
        assert_eq!(recovered.preferences, config.preferences);
        assert_eq!(recovered.match_settings, MatchSettings::default());
    }

    #[test]
    fn unknown_top_level_settings_fields_remain_rejected() {
        let config = ControlsConfig::default();
        let mut file: serde_json::Value = serde_json::from_str(&config.to_json().unwrap()).unwrap();
        file["unexpected"] = true.into();
        assert!(matches!(
            ControlsConfig::from_json(&file.to_string()),
            Err(BindingConfigError::InvalidJson(_))
        ));
    }

    #[test]
    fn corrupt_duplicate_and_unsupported_configuration_is_rejected_or_recovered() {
        let corrupt = "{ not json";
        assert!(matches!(
            ControlsConfig::from_json(corrupt),
            Err(BindingConfigError::InvalidJson(_))
        ));

        let config = ControlsConfig::default();
        let unsupported_version =
            config
                .to_json()
                .unwrap()
                .replacen("\"version\": 6", "\"version\": 7", 1);
        assert!(matches!(
            ControlsConfig::from_json(&unsupported_version),
            Err(BindingConfigError::UnsupportedVersion(7))
        ));

        let mut file: ControlsFile = serde_json::from_str(&config.to_json().unwrap()).unwrap();
        file.keyboard[1].action = file.keyboard[0].action;
        let (recovered, warning) =
            ControlsConfig::from_json(&serde_json::to_string(&file).unwrap()).unwrap();
        assert_eq!(
            warning.as_deref(),
            Some(
                "Keyboard bindings were invalid (duplicate action PlayerOneUp); using keyboard \
                 defaults."
            )
        );
        assert_eq!(recovered.keyboard, KeyboardBindings::default());
        assert_eq!(recovered.controller, config.controller);

        let unsupported_input = config
            .to_json()
            .unwrap()
            .replace("\"d_pad_up\"", "\"not_a_supported_input\"");
        let (recovered, warning) = ControlsConfig::from_json(&unsupported_input).unwrap();
        assert_eq!(
            warning.as_deref(),
            Some(
                "Controller bindings were invalid (unknown variant `not_a_supported_input`); \
                 using controller defaults."
            ),
            "the cause without serde's list of every accepted name"
        );
        assert_eq!(recovered.keyboard, config.keyboard);
        assert_eq!(recovered.controller, ControllerBindings::default());
    }

    #[test]
    fn oversized_settings_are_rejected_before_deserialization() {
        let oversized = " ".repeat(MAX_SETTINGS_JSON_BYTES + 1);
        assert!(matches!(
            ControlsConfig::from_json(&oversized),
            Err(BindingConfigError::DataTooLarge)
        ));
    }
}
