//! Validated match configuration shared by persistence, gameplay, and Replays.
//!
//! Every optional rule stores its enabled flag separately from its value. This
//! keeps a player's configured value while the rule is disabled. Durations are
//! always whole seconds; the minute/second split is presentation-only.
//!
//! One serde shape, two files: [`MatchSettings`] is written into
//! `controls.json`'s `game` section by the `client`-only `settings` module,
//! and, as the immutable snapshot a Match was played under, into every Replay
//! from [`crate::replay::MATCH_RULES_REPLAY_SCHEMA_VERSION`] on. Both readers
//! go through the private `RawMatchSettings` below, which is
//! `deny_unknown_fields` with every field required and no defaults, so adding
//! a rule here is a settings schema change *and* a Replay schema change at
//! once: without a new version of each and a default for the missing field, an
//! existing `game` section falls back to defaults with a warning and every
//! existing Replay fails to load. See `docs/ARCHITECTURE.md`'s "Change a
//! persisted schema" recipe.

#[cfg(feature = "client")]
use std::fmt;

#[cfg(feature = "client")]
use bevy::prelude::Resource;
use serde::{Deserialize, Deserializer, Serialize};

// The inclusive range each optional rule's value must fall in. User settings
// are clamped into these on load and on every edit; a Replay's snapshot is
// rejected outright instead, in `from_replay_value_strict`. The `_SECONDS`
// pair bounds both Round Time and Match Time.
pub const ROUND_LIMIT_MIN: u16 = 1;
pub const ROUND_LIMIT_MAX: u16 = 1_000;
pub const DURATION_MIN_SECONDS: u16 = 1;
pub const DURATION_MAX_SECONDS: u16 = 3_600;
pub const NO_STONE_ROUND_LIMIT_MIN: u16 = 1;
pub const NO_STONE_ROUND_LIMIT_MAX: u16 = 1_000;
pub const POINT_TARGET_MIN: u16 = 1;
pub const POINT_TARGET_MAX: u16 = 81;

const DEFAULT_ROUND_LIMIT: u16 = 100;
const DEFAULT_ROUND_TIME_SECONDS: u16 = 60;
const DEFAULT_NO_STONE_ROUND_LIMIT: u16 = 10;
const DEFAULT_MATCH_TIME_SECONDS: u16 = 600;
const DEFAULT_POINT_TARGET: u16 = 10;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct EnabledValue {
    enabled: bool,
    value: u16,
}

impl EnabledValue {
    const fn new(value: u16) -> Self {
        Self {
            enabled: false,
            value,
        }
    }
}

/// A field whose numeric range is defined by the match-settings domain.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MatchSettingField {
    RoundLimit,
    RoundTime,
    NoStoneRoundLimit,
    MatchTime,
    PointTarget,
}

impl MatchSettingField {
    /// The player-facing name of this rule, used by the Settings screen and
    /// by the range-rejection message a Replay reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::RoundLimit => "Round Limit",
            Self::RoundTime => "Round Time",
            Self::NoStoneRoundLimit => "End After Rounds Without a Board Change",
            Self::MatchTime => "Match Time",
            Self::PointTarget => "Point Target",
        }
    }

    /// The largest value this rule accepts. The minimum is not a constant of
    /// the field alone: Match Time's depends on Round Time, which is why it
    /// is [`MatchSettings::match_time_minimum`].
    pub const fn maximum(self) -> u16 {
        match self {
            Self::RoundLimit => ROUND_LIMIT_MAX,
            Self::RoundTime | Self::MatchTime => DURATION_MAX_SECONDS,
            Self::NoStoneRoundLimit => NO_STONE_ROUND_LIMIT_MAX,
            Self::PointTarget => POINT_TARGET_MAX,
        }
    }
}

/// A parse failure for an editor field.
#[cfg(feature = "client")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NumericParseError {
    Empty,
    NotDigits,
}

#[cfg(feature = "client")]
impl fmt::Display for NumericParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(formatter, "enter a whole number"),
            Self::NotDigits => write!(formatter, "use digits only"),
        }
    }
}

/// Validated match rules. Private fields prevent invalid construction.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "client", derive(Resource))]
#[serde(deny_unknown_fields)]
pub struct MatchSettings {
    round_limit: EnabledValue,
    round_time: EnabledValue,
    no_stone_round_limit: EnabledValue,
    match_time: EnabledValue,
    point_target: EnabledValue,
    capture_points: bool,
}

impl Default for MatchSettings {
    fn default() -> Self {
        Self {
            round_limit: EnabledValue::new(DEFAULT_ROUND_LIMIT),
            round_time: EnabledValue::new(DEFAULT_ROUND_TIME_SECONDS),
            no_stone_round_limit: EnabledValue::new(DEFAULT_NO_STONE_ROUND_LIMIT),
            match_time: EnabledValue::new(DEFAULT_MATCH_TIME_SECONDS),
            point_target: EnabledValue::new(DEFAULT_POINT_TARGET),
            capture_points: false,
        }
    }
}

impl MatchSettings {
    pub const fn enabled(&self, field: MatchSettingField) -> bool {
        self.setting(field).enabled
    }

    pub const fn value(&self, field: MatchSettingField) -> u16 {
        self.setting(field).value
    }

    pub const fn capture_points(&self) -> bool {
        self.capture_points
    }

    /// Parses the snapshot a Replay carries from
    /// [`crate::replay::MATCH_RULES_REPLAY_SCHEMA_VERSION`] on, without
    /// normalizing hostile data. `replay::compat` also runs it over an older
    /// file that unexpectedly carries the field, so that only the
    /// all-disabled default is accepted there.
    ///
    /// User settings intentionally clamp values while loading so a typo does
    /// not make the whole controls file unusable. Replays are immutable
    /// historical records, so their snapshot must instead be rejected if any
    /// value is outside the domain or violates the duration invariant.
    pub fn from_replay_value_strict(value: serde_json::Value) -> Result<Self, String> {
        let raw: RawMatchSettings =
            serde_json::from_value(value).map_err(|error| error.to_string())?;
        Self::validate_raw_value(
            MatchSettingField::RoundLimit,
            raw.round_limit.value,
            ROUND_LIMIT_MIN,
        )?;
        Self::validate_raw_value(
            MatchSettingField::RoundTime,
            raw.round_time.value,
            DURATION_MIN_SECONDS,
        )?;
        Self::validate_raw_value(
            MatchSettingField::NoStoneRoundLimit,
            raw.no_stone_round_limit.value,
            NO_STONE_ROUND_LIMIT_MIN,
        )?;
        Self::validate_raw_value(
            MatchSettingField::MatchTime,
            raw.match_time.value,
            DURATION_MIN_SECONDS,
        )?;
        Self::validate_raw_value(
            MatchSettingField::PointTarget,
            raw.point_target.value,
            POINT_TARGET_MIN,
        )?;
        if raw.round_time.enabled && raw.match_time.value < raw.round_time.value {
            return Err("match time is shorter than enabled round time".into());
        }

        Ok(Self {
            round_limit: EnabledValue {
                enabled: raw.round_limit.enabled,
                value: raw.round_limit.value as u16,
            },
            round_time: EnabledValue {
                enabled: raw.round_time.enabled,
                value: raw.round_time.value as u16,
            },
            no_stone_round_limit: EnabledValue {
                enabled: raw.no_stone_round_limit.enabled,
                value: raw.no_stone_round_limit.value as u16,
            },
            match_time: EnabledValue {
                enabled: raw.match_time.enabled,
                value: raw.match_time.value as u16,
            },
            point_target: EnabledValue {
                enabled: raw.point_target.enabled,
                value: raw.point_target.value as u16,
            },
            capture_points: raw.capture_points,
        })
    }

    pub fn set_enabled(&mut self, field: MatchSettingField, enabled: bool) {
        self.setting_mut(field).enabled = enabled;
        self.reconcile_duration_minimum();
    }

    fn validate_raw_value(
        field: MatchSettingField,
        value: u64,
        minimum: u16,
    ) -> Result<(), String> {
        if !(u64::from(minimum)..=u64::from(field.maximum())).contains(&value) {
            return Err(format!("{} is outside its supported range", field.label()));
        }
        Ok(())
    }

    pub fn toggle_enabled(&mut self, field: MatchSettingField) {
        let enabled = !self.enabled(field);
        self.set_enabled(field, enabled);
    }

    pub fn toggle_capture_points(&mut self) {
        self.set_capture_points(!self.capture_points);
    }

    pub fn set_capture_points(&mut self, enabled: bool) {
        self.capture_points = enabled;
    }

    /// Stores a value after applying the field's range and duration invariant.
    ///
    /// Returns the stored value. Raising Round Time can also raise Match Time;
    /// callers can simply refresh all visible fields after this method.
    pub fn set_value(&mut self, field: MatchSettingField, value: u64) -> u16 {
        let clamped = clamp_value(field, value, self.match_time_minimum());
        self.setting_mut(field).value = clamped;
        self.reconcile_duration_minimum();
        self.value(field)
    }

    /// The smallest Match Time these settings allow: an enabled Round Time's
    /// value, since a Match may not be shorter than one of its own rounds,
    /// and [`DURATION_MIN_SECONDS`] otherwise. [`Self::set_value`] and
    /// [`Self::set_enabled`] re-apply it, so the two durations never cross.
    pub const fn match_time_minimum(&self) -> u16 {
        if self.round_time.enabled {
            self.round_time.value
        } else {
            DURATION_MIN_SECONDS
        }
    }

    fn reconcile_duration_minimum(&mut self) {
        self.round_time.value = self
            .round_time
            .value
            .clamp(DURATION_MIN_SECONDS, DURATION_MAX_SECONDS);
        let minimum = self.match_time_minimum();
        self.match_time.value = self.match_time.value.clamp(minimum, DURATION_MAX_SECONDS);
    }

    const fn setting(&self, field: MatchSettingField) -> &EnabledValue {
        match field {
            MatchSettingField::RoundLimit => &self.round_limit,
            MatchSettingField::RoundTime => &self.round_time,
            MatchSettingField::NoStoneRoundLimit => &self.no_stone_round_limit,
            MatchSettingField::MatchTime => &self.match_time,
            MatchSettingField::PointTarget => &self.point_target,
        }
    }

    fn setting_mut(&mut self, field: MatchSettingField) -> &mut EnabledValue {
        match field {
            MatchSettingField::RoundLimit => &mut self.round_limit,
            MatchSettingField::RoundTime => &mut self.round_time,
            MatchSettingField::NoStoneRoundLimit => &mut self.no_stone_round_limit,
            MatchSettingField::MatchTime => &mut self.match_time,
            MatchSettingField::PointTarget => &mut self.point_target,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEnabledValue {
    enabled: bool,
    value: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMatchSettings {
    round_limit: RawEnabledValue,
    round_time: RawEnabledValue,
    no_stone_round_limit: RawEnabledValue,
    match_time: RawEnabledValue,
    point_target: RawEnabledValue,
    capture_points: bool,
}

impl<'de> Deserialize<'de> for MatchSettings {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawMatchSettings::deserialize(deserializer)?;
        let mut settings = Self::default();
        settings.round_limit.enabled = raw.round_limit.enabled;
        settings.round_limit.value = clamp_value(
            MatchSettingField::RoundLimit,
            raw.round_limit.value,
            DURATION_MIN_SECONDS,
        );
        settings.round_time.enabled = raw.round_time.enabled;
        settings.round_time.value = clamp_value(
            MatchSettingField::RoundTime,
            raw.round_time.value,
            DURATION_MIN_SECONDS,
        );
        settings.no_stone_round_limit.enabled = raw.no_stone_round_limit.enabled;
        settings.no_stone_round_limit.value = clamp_value(
            MatchSettingField::NoStoneRoundLimit,
            raw.no_stone_round_limit.value,
            DURATION_MIN_SECONDS,
        );
        settings.match_time.enabled = raw.match_time.enabled;
        settings.match_time.value = clamp_value(
            MatchSettingField::MatchTime,
            raw.match_time.value,
            settings.match_time_minimum(),
        );
        settings.point_target.enabled = raw.point_target.enabled;
        settings.point_target.value = clamp_value(
            MatchSettingField::PointTarget,
            raw.point_target.value,
            DURATION_MIN_SECONDS,
        );
        settings.capture_points = raw.capture_points;
        settings.reconcile_duration_minimum();
        Ok(settings)
    }
}

fn clamp_value(field: MatchSettingField, value: u64, match_time_minimum: u16) -> u16 {
    let minimum = match field {
        MatchSettingField::RoundLimit => ROUND_LIMIT_MIN,
        MatchSettingField::RoundTime => DURATION_MIN_SECONDS,
        MatchSettingField::NoStoneRoundLimit => NO_STONE_ROUND_LIMIT_MIN,
        MatchSettingField::MatchTime => match_time_minimum,
        MatchSettingField::PointTarget => POINT_TARGET_MIN,
    } as u64;
    value.clamp(minimum, u64::from(field.maximum())) as u16
}

// The helpers below turn a player's typed text into a validated field value,
// and back into the minute/second pair the editor displays. Only the Settings
// screen calls them, and a headless consumer has no text input at all, so they
// are `client`-only rather than part of the headless core. The contrast is
// with `crate::authority`'s Match-clock arithmetic - `advance_clock` and the
// private deadline helpers in `authority::timing` - which looked equally
// client-facing while both were gated, but is headless because a headless
// consumer really does advance a Match clock. None of that arithmetic has
// ever lived here.

#[cfg(feature = "client")]
pub(crate) fn field_minimum(field: MatchSettingField, settings: &MatchSettings) -> u16 {
    match field {
        MatchSettingField::RoundLimit => ROUND_LIMIT_MIN,
        MatchSettingField::RoundTime => DURATION_MIN_SECONDS,
        MatchSettingField::NoStoneRoundLimit => NO_STONE_ROUND_LIMIT_MIN,
        MatchSettingField::MatchTime => settings.match_time_minimum(),
        MatchSettingField::PointTarget => POINT_TARGET_MIN,
    }
}

/// Parses an unsigned decimal editor value, rejecting every non-digit form and
/// clamping the numeric result to the supplied inclusive bounds.
#[cfg(feature = "client")]
pub(crate) fn parse_digits_clamped(
    text: &str,
    minimum: u16,
    maximum: u16,
) -> Result<u16, NumericParseError> {
    if text.is_empty() {
        return Err(NumericParseError::Empty);
    }
    if !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(NumericParseError::NotDigits);
    }
    let value = text
        .bytes()
        .fold(0_u64, |value, digit| {
            value
                .saturating_mul(10)
                .saturating_add(u64::from(digit - b'0'))
        })
        .clamp(u64::from(minimum), u64::from(maximum));
    Ok(value as u16)
}

/// Parses and normalizes a `minutes:seconds` editor pair as whole seconds.
#[cfg(feature = "client")]
pub(crate) fn parse_duration_parts(
    minutes: &str,
    seconds: &str,
    minimum_seconds: u16,
) -> Result<u16, NumericParseError> {
    let minutes = parse_unbounded_digits(minutes)?;
    let seconds = parse_unbounded_digits(seconds)?;
    let total = minutes.saturating_mul(60).saturating_add(seconds);
    Ok(total.clamp(u64::from(minimum_seconds), u64::from(DURATION_MAX_SECONDS)) as u16)
}

#[cfg(feature = "client")]
fn parse_unbounded_digits(text: &str) -> Result<u64, NumericParseError> {
    if text.is_empty() {
        return Err(NumericParseError::Empty);
    }
    if !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(NumericParseError::NotDigits);
    }
    Ok(text.bytes().fold(0_u64, |value, digit| {
        value
            .saturating_mul(10)
            .saturating_add(u64::from(digit - b'0'))
    }))
}

#[cfg(feature = "client")]
pub(crate) const fn duration_parts(total_seconds: u16) -> (u16, u16) {
    (total_seconds / 60, total_seconds % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_retain_sensible_values_with_every_rule_disabled() {
        let settings = MatchSettings::default();
        for field in [
            MatchSettingField::RoundLimit,
            MatchSettingField::RoundTime,
            MatchSettingField::NoStoneRoundLimit,
            MatchSettingField::MatchTime,
            MatchSettingField::PointTarget,
        ] {
            assert!(!settings.enabled(field));
        }
        assert_eq!(settings.value(MatchSettingField::RoundLimit), 100);
        assert_eq!(settings.value(MatchSettingField::RoundTime), 60);
        assert_eq!(settings.value(MatchSettingField::NoStoneRoundLimit), 10);
        assert_eq!(settings.value(MatchSettingField::MatchTime), 600);
        assert_eq!(settings.value(MatchSettingField::PointTarget), 10);
        assert!(!settings.capture_points());
    }

    #[cfg(feature = "client")]
    #[test]
    fn digit_parser_rejects_non_integer_forms_and_clamps_bounds() {
        assert_eq!(
            parse_digits_clamped("", 1, 1_000),
            Err(NumericParseError::Empty)
        );
        for invalid in ["-1", "+1", "1.0", "1e2", "12x", " 12"] {
            assert_eq!(
                parse_digits_clamped(invalid, 1, 1_000),
                Err(NumericParseError::NotDigits),
                "{invalid}"
            );
        }
        assert_eq!(parse_digits_clamped("0", 1, 1_000), Ok(1));
        assert_eq!(parse_digits_clamped("5000", 1, 1_000), Ok(1_000));
        assert_eq!(parse_digits_clamped("999999999999999999999", 1, 81), Ok(81));
    }

    #[cfg(feature = "client")]
    #[test]
    fn duration_parts_normalize_and_support_exactly_sixty_minutes() {
        assert_eq!(parse_duration_parts("1", "90", 1), Ok(150));
        assert_eq!(duration_parts(150), (2, 30));
        assert_eq!(parse_duration_parts("60", "00", 1), Ok(3_600));
        assert_eq!(duration_parts(3_600), (60, 0));
        assert_eq!(parse_duration_parts("60", "01", 1), Ok(3_600));
        assert_eq!(parse_duration_parts("0", "0", 1), Ok(1));
    }

    #[test]
    fn enabled_round_time_defines_match_time_minimum() {
        let mut settings = MatchSettings::default();
        settings.set_value(MatchSettingField::MatchTime, 30);
        assert_eq!(settings.value(MatchSettingField::MatchTime), 30);
        settings.set_enabled(MatchSettingField::RoundTime, true);
        assert_eq!(settings.match_time_minimum(), 60);
        assert_eq!(settings.value(MatchSettingField::MatchTime), 60);
        settings.set_value(MatchSettingField::RoundTime, 120);
        assert_eq!(settings.value(MatchSettingField::MatchTime), 120);
        // The reconciliation above is headless; only this last line - that the
        // editor clamps a typed value up to that minimum - needs `client`.
        #[cfg(feature = "client")]
        assert_eq!(parse_duration_parts("0", "10", 120), Ok(120));
    }

    #[test]
    fn serde_clamps_untrusted_values_without_allowing_invalid_state() {
        let json = r#"{
            "round_limit":{"enabled":true,"value":0},
            "round_time":{"enabled":true,"value":4000},
            "no_stone_round_limit":{"enabled":true,"value":9999},
            "match_time":{"enabled":true,"value":1},
            "point_target":{"enabled":true,"value":0},
            "capture_points":true
        }"#;
        let loaded: MatchSettings = serde_json::from_str(json).unwrap();
        assert_eq!(loaded.value(MatchSettingField::RoundLimit), 1);
        assert_eq!(loaded.value(MatchSettingField::RoundTime), 3_600);
        assert_eq!(loaded.value(MatchSettingField::NoStoneRoundLimit), 1_000);
        assert_eq!(loaded.value(MatchSettingField::MatchTime), 3_600);
        assert_eq!(loaded.value(MatchSettingField::PointTarget), 1);
        assert!(loaded.capture_points());
        assert!(!serde_json::to_string(&loaded).unwrap().contains('.'));
    }
}
