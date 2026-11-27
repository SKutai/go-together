//! The two overlays the viewer can open: the Key, which names the replay-only
//! visuals, and Match Settings, which shows the rules the recorded match was
//! played under.
//!
//! Both are spawned hidden along with the rest of the screen by
//! [`super::layout`] and are then only shown or hidden by [`super::display`],
//! so opening one never spawns anything. That they are mutually exclusive and
//! that either one pauses playback are transport rules, and stay with the
//! transport systems in [`super`].
//!
//! They are a separate module from [`super::layout`] for their size rather
//! than their kind: between them they are nearly half of the viewer's spawn
//! code, and none of it has anything to do with drawing the board.

use bevy::prelude::*;

use super::geometry::{VIEWER_MATCH_SETTINGS_MODAL_MAX_HEIGHT, VIEWER_MATCH_SETTINGS_MODAL_WIDTH};
use super::layout::spawn_viewer_button;
use super::{
    ViewerAction, ViewerKeyModal, ViewerMatchSettingsModal, format_setting_duration, player_index,
};
use crate::app::visuals::stone_geometry;
use crate::match_settings::MatchSettingField;
use crate::presentation::{
    BLACK_STONE, BOARD_WOOD, GRAY_STONE, ORANGE, OVERLAY_BACKGROUND, PANEL_BACKGROUND,
    SCORE_BADGE_BACKGROUND, STONE_RING, TEXT_HEADING, TEXT_MUTED, TEXT_PRIMARY, WHITE_STONE,
    hidden_selection_strokes, player_accent, teleport_marker_strokes,
};
use crate::replay::{EndReason, Replay};
use crate::rules::{Cell, Player};
pub(super) fn spawn_viewer_key_modal(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn((
            // A plain node: picking stops at it, so it blocks every click
            // to the screen beneath (`ui::button`).
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                display: Display::None,
                ..default()
            },
            BackgroundColor(OVERLAY_BACKGROUND),
            GlobalZIndex(200),
            ViewerKeyModal,
        ))
        .with_children(|overlay| {
            overlay
                .spawn((
                    Node {
                        width: px(720),
                        padding: UiRect::all(px(24)),
                        border: UiRect::all(px(2)),
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Center,
                        row_gap: px(13),
                        ..default()
                    },
                    BackgroundColor(PANEL_BACKGROUND),
                    BorderColor::all(ORANGE),
                ))
                .with_children(|modal| {
                    modal.spawn((
                        Text::new("Replay Key"),
                        TextFont {
                            font_size: FontSize::Px(30.0),
                            ..default()
                        },
                        TextColor(TEXT_HEADING),
                        TextLayout::justify(Justify::Center),
                    ));
                    modal.spawn((
                        Text::new(
                            "These colors and shapes identify each player and the board state during a Replay.",
                        ),
                        TextFont {
                            font_size: FontSize::Px(16.0),
                            ..default()
                        },
                        TextColor(TEXT_PRIMARY),
                        TextLayout::justify(Justify::Center),
                    ));
                    spawn_key_row(
                        modal,
                        KeySwatch::Cursor {
                            fill: player_accent(Player::Two),
                            border: WHITE_STONE,
                        },
                        "White / P2 cursor: blue with a white border.",
                    );
                    spawn_key_row(
                        modal,
                        KeySwatch::Cursor {
                            fill: player_accent(Player::One),
                            border: BLACK_STONE,
                        },
                        "Black / P1 cursor: red with a black border.",
                    );
                    spawn_key_row(modal, KeySwatch::ScoreBadges, KEY_SCORE_BADGE_SENTENCE);
                    // White / P2 before Black / P1, as the cursor rows are.
                    for player in [Player::Two, Player::One] {
                        spawn_key_row(
                            modal,
                            KeySwatch::Marker(player),
                            KEY_MARKER_SENTENCES[player_index(player)],
                        );
                    }
                    for player in [Player::Two, Player::One] {
                        spawn_key_row(
                            modal,
                            KeySwatch::HiddenChoice(player),
                            KEY_HIDDEN_CHOICE_SENTENCES[player_index(player)],
                        );
                    }
                    spawn_key_row(
                        modal,
                        KeySwatch::Stone(Cell::Black),
                        "Black stones belong to Black / P1.",
                    );
                    spawn_key_row(
                        modal,
                        KeySwatch::Stone(Cell::White),
                        "White stones have a dark ring and belong to White / P2.",
                    );
                    spawn_key_row(
                        modal,
                        KeySwatch::Stone(Cell::Gray),
                        "Gray stones are smaller, with a dark ring. They are neutral and permanent, and provide no liberties - like the board edge.",
                    );
                    modal
                        .spawn(Node {
                            width: percent(100),
                            justify_content: JustifyContent::Center,
                            margin: UiRect::top(px(4)),
                            ..default()
                        })
                        .with_children(|footer| {
                            spawn_viewer_button(footer, ViewerAction::CloseKey, "Close", 120.0);
                        });
                });
        });
}

pub(super) fn spawn_viewer_match_settings_modal(
    parent: &mut ChildSpawnerCommands,
    replay: &Replay,
) {
    parent
        .spawn((
            // A plain node: picking stops at it, so it blocks every click
            // to the screen beneath (`ui::button`).
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                display: Display::None,
                ..default()
            },
            BackgroundColor(OVERLAY_BACKGROUND),
            GlobalZIndex(200),
            ViewerMatchSettingsModal,
        ))
        .with_children(|overlay| {
            overlay
                .spawn((
                    Node {
                        width: px(VIEWER_MATCH_SETTINGS_MODAL_WIDTH),
                        max_height: px(VIEWER_MATCH_SETTINGS_MODAL_MAX_HEIGHT),
                        padding: UiRect::all(px(28)),
                        border: UiRect::all(px(2)),
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Center,
                        row_gap: px(12),
                        ..default()
                    },
                    BackgroundColor(PANEL_BACKGROUND),
                    BorderColor::all(ORANGE),
                ))
                .with_children(|modal| {
                    modal.spawn((
                        Text::new("Match Settings"),
                        TextFont {
                            font_size: FontSize::Px(30.0),
                            ..default()
                        },
                        TextColor(TEXT_HEADING),
                        TextLayout::justify(Justify::Center),
                    ));
                    modal.spawn((
                        Text::new("Immutable rules recorded with this Replay"),
                        TextFont {
                            font_size: FontSize::Px(15.0),
                            ..default()
                        },
                        TextColor(TEXT_MUTED),
                        TextLayout::justify(Justify::Center),
                    ));
                    for (label, value) in match_settings_rows(replay) {
                        spawn_match_settings_row(modal, label, value);
                    }
                    modal.spawn((
                        Text::new("End Reasons"),
                        TextFont {
                            font_size: FontSize::Px(18.0),
                            ..default()
                        },
                        TextColor(TEXT_HEADING),
                        TextLayout::justify(Justify::Center),
                        Node {
                            margin: UiRect::top(px(6)),
                            ..default()
                        },
                    ));
                    for reason in replay_end_reason_labels(replay) {
                        modal.spawn((
                            Text::new(reason),
                            TextFont {
                                font_size: FontSize::Px(16.0),
                                ..default()
                            },
                            TextColor(TEXT_PRIMARY),
                            TextLayout::justify(Justify::Center),
                        ));
                    }
                    modal
                        .spawn(Node {
                            width: percent(100),
                            justify_content: JustifyContent::Center,
                            margin: UiRect::top(px(6)),
                            ..default()
                        })
                        .with_children(|footer| {
                            spawn_viewer_button(
                                footer,
                                ViewerAction::CloseMatchSettings,
                                "Close",
                                120.0,
                            );
                        });
                });
        });
}

fn spawn_match_settings_row(parent: &mut ChildSpawnerCommands, label: &'static str, value: String) {
    parent
        .spawn(Node {
            width: percent(100),
            align_items: AlignItems::Center,
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                Text::new(label),
                TextFont {
                    font_size: FontSize::Px(16.0),
                    ..default()
                },
                TextColor(TEXT_PRIMARY),
                Node {
                    flex_grow: 1.0,
                    ..default()
                },
            ));
            row.spawn((
                Text::new(value),
                TextFont {
                    font_size: FontSize::Px(16.0),
                    ..default()
                },
                TextColor(TEXT_HEADING),
            ));
        });
}

fn match_settings_rows(replay: &Replay) -> Vec<(&'static str, String)> {
    let settings = &replay.match_settings;
    [
        MatchSettingField::RoundLimit,
        MatchSettingField::RoundTime,
        MatchSettingField::NoStoneRoundLimit,
        MatchSettingField::MatchTime,
        MatchSettingField::PointTarget,
    ]
    .into_iter()
    .map(|field| {
        let value = if !settings.enabled(field) {
            "Off".into()
        } else if matches!(
            field,
            MatchSettingField::RoundTime | MatchSettingField::MatchTime
        ) {
            format_setting_duration(settings.value(field))
        } else {
            settings.value(field).to_string()
        };
        (field.label(), value)
    })
    .chain(std::iter::once((
        "Capture Points",
        if settings.capture_points() {
            "On".into()
        } else {
            "Off".into()
        },
    )))
    .collect()
}

fn replay_end_reason_labels(replay: &Replay) -> Vec<&'static str> {
    [
        EndReason::Manual,
        EndReason::MatchTimeLimit,
        EndReason::RoundLimit,
        EndReason::NoStoneRoundLimit,
        EndReason::PointTarget,
        EndReason::Legacy,
    ]
    .into_iter()
    .filter(|reason| replay.end_reasons.contains(reason))
    .map(EndReason::label)
    .collect()
}

/// What the Key says of each player's Teleport Marker and of their hidden
/// selections, Player One's first. Each names the mark's shape and which
/// way round its dark and light strokes run, because those - not the hue -
/// are what tell the two players' marks apart; the hue it names is the one
/// stroke that looks it, the accent. Player Two's marker is described in
/// the words their hidden selections are, since it is the same three
/// strokes; Player One's is black either side of its red, which "a black
/// border" says.
const KEY_MARKER_SENTENCES: [&str; 2] = [
    "Black / P1 Teleport Marker: a larger red diamond with a black border, shown only in Replays.",
    "White / P2 Teleport Marker: a smaller blue diamond, white outside and dark inside, shown only in Replays.",
];
const KEY_HIDDEN_CHOICE_SENTENCES: [&str; 2] = [
    "Black / P1's hidden choices (up to three in Round 0): a hollow red ring, black outside and light inside.",
    "White / P2's hidden choices (up to three in Round 0): a hollow blue ring, white outside and dark inside.",
];
/// What the Key says of the score badges. A badge trails its own cursor,
/// which says whose it is, but when both cursors share a point the two sit
/// side by side and differ only in the hue of their frames - one colour to
/// a player who cannot see hue - so their order is what tells them apart,
/// and this is where the game says what that order is. It names the order
/// [`super::geometry::score_badge_position`] draws, which is the order the
/// result line at the top of the screen names the players in;
/// `the_keys_badge_row_names_the_order_the_board_draws` holds the sentence
/// and the swatch beside it to the board.
const KEY_SCORE_BADGE_SENTENCE: &str = "Score badges show each player's score beside their cursor. When both cursors share a point, Black / P1's is on the left, as in the score at the top.";

/// What a Key row shows beside its sentence.
#[derive(Clone, Copy, Debug, PartialEq)]
enum KeySwatch {
    /// A cursor: a square in the outline's two colours, drawn straight on
    /// the modal's panel.
    Cursor { fill: Color, border: Color },
    /// A stone as both boards draw it - on board wood, at its own scale and
    /// with its ring if it carries one - so the Key shows the ring instead
    /// of only naming it.
    Stone(Cell),
    /// A player's Teleport Marker: the diamond in its three strokes, on
    /// board wood, Player Two's the smaller as on the board.
    Marker(Player),
    /// A player's hidden-selection mark: its three strokes around a bare
    /// middle, on board wood like the stones it is told apart from.
    HiddenChoice(Player),
    /// Both players' score badges side by side, on board wood, in the
    /// order the board puts them in when the cursors share a point.
    ScoreBadges,
}

/// Side of a swatch, and of the wood chip every swatch but a cursor's sits
/// on: enough for Player One's marker swatch to stand clearly larger than
/// Player Two's, which the sentences beside them say.
const KEY_SWATCH_SIZE: f32 = 30.0;
/// Diameter of a black or white stone on its chip; the other stones scale
/// from it exactly as they do on the board.
const KEY_STONE_DIAMETER: f32 = 24.0;
/// Side of each player's Teleport Marker swatch before it is turned on its
/// point, Player One's first. A square on its point is the square root of
/// two times as wide as its side, so 20 spans 28.3 of the chip's 30; every
/// size here is even so that it centres on its chip to the pixel. Player
/// Two's is smaller, as on the board, though by less than the board's
/// half: three strokes of two pixels leave a side of 16 a bare middle of
/// four, and a side of 14 only two, under the four the test asks for.
const KEY_MARKER_SIDES: [f32; 2] = [20.0, 16.0];
/// Diameter of a hidden-selection swatch.
const KEY_HIDDEN_CHOICE_DIAMETER: f32 = 20.0;
/// Width of each of the three strokes of a marker or hidden-selection
/// swatch. The strokes are wider than the board's proportion, which at
/// this size would leave none of them two pixels.
const KEY_MARK_STROKE: f32 = 2.0;
/// Width and height of each badge in the score badges' swatch, its frame,
/// and the wood left between the two. Two of the board's badges, 42 wide,
/// cannot share a chip of 30, so these keep the board's proportion of
/// three to two, its two-pixel frame and its inks, and not its size; the
/// pair spans 26 and so centres on its chip to the pixel.
const KEY_BADGE_SIZE: Vec2 = Vec2::new(12.0, 8.0);
const KEY_BADGE_FRAME: f32 = 2.0;
const KEY_BADGE_GAP: f32 = 2.0;

/// Fill, diameter and ring width of a stone swatch, in pixels.
///
/// The ring is the stone's footprint less its fill, as on the board, but
/// never thinner than two pixels: at this size the board's proportion would
/// round to a hairline.
fn key_stone_swatch(cell: Cell) -> (Color, f32, f32) {
    let fill = match cell {
        Cell::Empty => BOARD_WOOD,
        Cell::Black => BLACK_STONE,
        Cell::White => WHITE_STONE,
        Cell::Gray => GRAY_STONE,
    };
    let geometry = stone_geometry(cell);
    match geometry.ring_scale {
        Some(ring_scale) => (fill, (ring_scale * KEY_STONE_DIAMETER).round(), 2.0),
        None => (
            fill,
            (geometry.fill_scale * KEY_STONE_DIAMETER).round(),
            0.0,
        ),
    }
}

fn spawn_key_row(parent: &mut ChildSpawnerCommands, swatch: KeySwatch, label: &str) {
    parent
        .spawn(Node {
            width: percent(100),
            align_items: AlignItems::Center,
            column_gap: px(14),
            ..default()
        })
        .with_children(|row| {
            match swatch {
                KeySwatch::Cursor { fill, border } => {
                    row.spawn((
                        Node {
                            width: px(KEY_SWATCH_SIZE),
                            height: px(KEY_SWATCH_SIZE),
                            border: UiRect::all(px(3)),
                            flex_shrink: 0.0,
                            ..default()
                        },
                        BackgroundColor(fill),
                        BorderColor::all(border),
                    ));
                }
                KeySwatch::Stone(cell) => {
                    let (fill, diameter, ring) = key_stone_swatch(cell);
                    spawn_key_stone(row, fill, diameter, ring);
                }
                KeySwatch::Marker(player) => spawn_key_marker(row, player),
                KeySwatch::HiddenChoice(player) => spawn_key_hidden_choice(row, player),
                KeySwatch::ScoreBadges => spawn_key_score_badges(row),
            }
            row.spawn((
                Text::new(label),
                TextFont {
                    font_size: FontSize::Px(16.0),
                    ..default()
                },
                TextColor(TEXT_PRIMARY),
                TextLayout::justify(Justify::Left).with_linebreak(LineBreak::WordOrCharacter),
                Node {
                    min_width: px(0),
                    flex_grow: 1.0,
                    ..default()
                },
            ));
        });
}

/// A chip of board wood with whatever `mark` spawns centred on it. The
/// marks the Key shows are drawn on wood because wood is where the board
/// draws them: a dark stroke cannot be seen on the modal's dark panel.
fn spawn_key_chip(row: &mut ChildSpawnerCommands, mark: impl FnOnce(&mut ChildSpawnerCommands)) {
    row.spawn((
        Node {
            width: px(KEY_SWATCH_SIZE),
            height: px(KEY_SWATCH_SIZE),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            flex_shrink: 0.0,
            ..default()
        },
        BackgroundColor(BOARD_WOOD),
    ))
    .with_children(mark);
}

/// One round stone centred on a chip of board wood.
fn spawn_key_stone(row: &mut ChildSpawnerCommands, fill: Color, diameter: f32, ring: f32) {
    spawn_key_chip(row, |chip| {
        chip.spawn((
            Node {
                width: px(diameter),
                height: px(diameter),
                border: UiRect::all(px(ring)),
                border_radius: BorderRadius::all(percent(50)),
                ..default()
            },
            BackgroundColor(fill),
            BorderColor::all(STONE_RING),
        ));
    });
}

/// The strokes of one player's Teleport Marker swatch, as [`key_stroke`]
/// takes them, outermost first.
fn key_marker_strokes(player: Player) -> [(Color, f32, f32); 3] {
    key_mark_strokes(
        teleport_marker_strokes(player),
        KEY_MARKER_SIDES[player_index(player)],
    )
}

/// The strokes of one player's hidden-selection swatch, outermost first.
fn key_hidden_choice_strokes(player: Player) -> [(Color, f32, f32); 3] {
    key_mark_strokes(hidden_selection_strokes(player), KEY_HIDDEN_CHOICE_DIAMETER)
}

/// The three strokes of a mark `size` across, in `inks` outermost first,
/// as [`key_stroke`] takes them. Each reaches from its own outer edge in
/// to the mark's inner edge, and a later one is painted over an earlier,
/// so the three never abut and no seam of wood shows between two - which
/// is how the board draws both marks too.
fn key_mark_strokes(inks: [Color; 3], size: f32) -> [(Color, f32, f32); 3] {
    let mut layer = 0.0;
    inks.map(|ink| {
        let stroke = (
            ink,
            size - 2.0 * layer * KEY_MARK_STROKE,
            (3.0 - layer) * KEY_MARK_STROKE,
        );
        layer += 1.0;
        stroke
    })
}

/// A player's Teleport Marker on a chip of board wood: a square outline
/// stood on its point, closed, as the board's is.
fn spawn_key_marker(row: &mut ChildSpawnerCommands, player: Player) {
    spawn_key_chip(row, |chip| {
        for (ink, side, width) in key_marker_strokes(player) {
            chip.spawn((
                key_stroke(ink, side, width, BorderRadius::ZERO),
                UiTransform::from_rotation(Rot2::FRAC_PI_4),
            ));
        }
    });
}

/// A player's hidden-selection mark on a chip of board wood.
fn spawn_key_hidden_choice(row: &mut ChildSpawnerCommands, player: Player) {
    spawn_key_chip(row, |chip| {
        for (ink, diameter, width) in key_hidden_choice_strokes(player) {
            chip.spawn(key_stroke(
                ink,
                diameter,
                width,
                BorderRadius::all(percent(50)),
            ));
        }
    });
}

/// Both players' score badges on a chip of board wood, side by side and in
/// flow, left to right in the order the board draws them on a shared point,
/// which puts Player One's first. Each is the board's dark tile in its
/// player's frame; half the gap either side of each puts the whole gap
/// between them.
fn spawn_key_score_badges(row: &mut ChildSpawnerCommands) {
    spawn_key_chip(row, |chip| {
        for player in [Player::One, Player::Two] {
            chip.spawn((
                Node {
                    width: px(KEY_BADGE_SIZE.x),
                    height: px(KEY_BADGE_SIZE.y),
                    border: UiRect::all(px(KEY_BADGE_FRAME)),
                    margin: UiRect::horizontal(px(KEY_BADGE_GAP * 0.5)),
                    ..default()
                },
                BackgroundColor(SCORE_BADGE_BACKGROUND),
                BorderColor::all(player_accent(player)),
            ));
        }
    });
}

/// One stroke of a Key mark: a node `size` across that is all border,
/// `width` wide, with nothing inside it. It is taken out of the chip's
/// flow, so that several are centred on one another rather than laid side
/// by side.
fn key_stroke(ink: Color, size: f32, width: f32, border_radius: BorderRadius) -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            width: px(size),
            height: px(size),
            border: UiRect::all(px(width)),
            border_radius,
            ..default()
        },
        BorderColor::all(ink),
    )
}

#[cfg(test)]
mod tests {
    use super::super::super::test_support::fixed_metadata_replay;
    use super::super::geometry::score_badge_position;
    use super::*;
    use crate::architecture::sources_in;

    #[test]
    fn match_settings_modal_formats_snapshot_and_reasons_in_canonical_order() {
        let mut replay = fixed_metadata_replay();
        replay
            .match_settings
            .set_value(MatchSettingField::RoundLimit, 250);
        replay
            .match_settings
            .set_enabled(MatchSettingField::RoundLimit, true);
        replay
            .match_settings
            .set_value(MatchSettingField::RoundTime, 3_600);
        replay
            .match_settings
            .set_enabled(MatchSettingField::RoundTime, true);
        replay
            .match_settings
            .set_value(MatchSettingField::NoStoneRoundLimit, 12);
        replay
            .match_settings
            .set_value(MatchSettingField::MatchTime, 3_600);
        replay
            .match_settings
            .set_enabled(MatchSettingField::MatchTime, true);
        replay
            .match_settings
            .set_value(MatchSettingField::PointTarget, 42);
        replay.match_settings.set_capture_points(true);
        replay.end_reasons = vec![EndReason::PointTarget, EndReason::RoundLimit];

        assert_eq!(
            match_settings_rows(&replay),
            [
                ("Round Limit", "250".into()),
                ("Round Time", "60:00".into()),
                ("End After Rounds Without a Board Change", "Off".into()),
                ("Match Time", "60:00".into()),
                ("Point Target", "Off".into()),
                ("Capture Points", "On".into()),
            ]
        );
        assert_eq!(
            replay_end_reason_labels(&replay),
            ["Round Limit", "Point Target"]
        );
    }

    #[test]
    fn legacy_match_settings_modal_shows_disabled_defaults_and_legacy_reason() {
        let mut replay = fixed_metadata_replay();
        replay.version = 4;
        replay.end_reasons = vec![EndReason::Legacy];

        assert!(
            match_settings_rows(&replay)
                .iter()
                .all(|(_, value)| value == "Off")
        );
        assert_eq!(replay_end_reason_labels(&replay), ["Legacy Match"]);
    }

    /// An ending both players agreed to is listed by the same label the
    /// Match screen shows under its result: [`EndReason::label`] is the one
    /// place either screen words a reason, so the two cannot drift apart.
    #[test]
    fn an_agreed_ending_is_listed_by_the_label_the_match_screen_shows() {
        let mut replay = fixed_metadata_replay();
        replay.end_reasons = vec![EndReason::Manual];
        assert_eq!(replay_end_reason_labels(&replay), ["Agreed Ending"]);
    }

    /// The Key draws each stone the way the boards do: gray smaller than
    /// black and white, and a ring on exactly the stones `stone_geometry`
    /// gives one.
    #[test]
    fn key_stone_swatches_take_their_ring_and_size_from_the_board() {
        let (black, black_diameter, black_ring) = key_stone_swatch(Cell::Black);
        let (white, white_diameter, white_ring) = key_stone_swatch(Cell::White);
        let (gray, gray_diameter, gray_ring) = key_stone_swatch(Cell::Gray);
        assert_eq!((black, white, gray), (BLACK_STONE, WHITE_STONE, GRAY_STONE));
        assert_eq!(black_ring, 0.0, "a black stone has no ring");
        assert!(white_ring >= 2.0 && gray_ring >= 2.0);
        assert_eq!(white_diameter, black_diameter);
        assert!(gray_diameter < white_diameter, "gray is drawn smaller");
        assert!(white_diameter <= KEY_SWATCH_SIZE);
        for cell in [Cell::Black, Cell::White, Cell::Gray] {
            let ringed = stone_geometry(cell).ring_scale.is_some();
            assert_eq!(key_stone_swatch(cell).2 > 0.0, ringed, "{cell:?}");
        }
    }

    /// The Key as it is actually spawned, read back from the world rather
    /// than from this file's text: a source scan for the two sentences
    /// would find them in the assertion itself and could never fail.
    #[test]
    fn the_spawned_key_shows_each_stones_ring_and_says_so() {
        let mut world = World::new();
        world
            .commands()
            .spawn(Node::default())
            .with_children(spawn_viewer_key_modal);
        world.flush();

        let sentences: Vec<String> = world
            .query::<&Text>()
            .iter(&world)
            .map(|text| text.0.clone())
            .collect();
        for expected in [
            "White stones have a dark ring and belong to White / P2.",
            "Gray stones are smaller, with a dark ring. They are neutral and permanent, \
             and provide no liberties - like the board edge.",
            "Black stones belong to Black / P1.",
        ] {
            assert!(
                sentences.iter().any(|sentence| sentence == expected),
                "the Key has no row reading {expected:?}; it reads {sentences:#?}"
            );
        }

        // Each stone swatch is the one round node filled in that stone's
        // colour. Its border is the ring: present, in the ring's ink, on
        // exactly the stones the boards ring.
        let mut swatches = world.query::<(&Node, &BackgroundColor, &BorderColor)>();
        for cell in [Cell::Black, Cell::White, Cell::Gray] {
            let (fill, diameter, ring) = key_stone_swatch(cell);
            let found: Vec<_> = swatches
                .iter(&world)
                .filter(|(_, background, _)| background.0 == fill)
                .collect();
            assert_eq!(found.len(), 1, "one swatch is filled in {cell:?}'s colour");
            let (node, _, border) = found[0];
            assert_eq!(node.width, px(diameter), "{cell:?}");
            assert_eq!(node.border, UiRect::all(px(ring)), "{cell:?}");
            assert_eq!(*border, BorderColor::all(STONE_RING), "{cell:?}");
            assert_eq!(node.border_radius, BorderRadius::all(percent(50)));
        }
        let wood_chips = swatches
            .iter(&world)
            .filter(|(_, background, _)| background.0 == BOARD_WOOD)
            .count();
        assert_eq!(
            wood_chips, 8,
            "three stones, two Teleport Markers, two hidden choices and the score badges sit on \
             wood"
        );
    }

    /// The Key's rows for the two Replay-only marks, read back from the
    /// spawned world row by row: each sentence names the mark's shape and
    /// which of its strokes is dark and which light, and the swatch beside
    /// it is that player's mark in the board's own three inks, its strokes
    /// painted outermost first so that every one of them shows.
    #[test]
    fn each_key_row_shows_the_mark_its_sentence_names() {
        let mut world = World::new();
        world
            .commands()
            .spawn(Node::default())
            .with_children(spawn_viewer_key_modal);
        world.flush();

        // A row is a node with two children, a swatch and a sentence; the
        // swatch of a mark is a wood chip whose children are its strokes,
        // in the order they are painted.
        let rows: Vec<(String, Entity)> = world
            .query::<&Children>()
            .iter(&world)
            .filter(|children| children.len() == 2)
            .filter_map(|children| {
                let sentence = world.get::<Text>(children[1])?.0.clone();
                Some((sentence, children[0]))
            })
            .collect();
        let strokes_beside = |expected: &str| -> Vec<(Color, f32, f32, BorderRadius, Rot2)> {
            let found: Vec<_> = rows
                .iter()
                .filter(|(sentence, _)| sentence == expected)
                .collect();
            assert_eq!(
                found.len(),
                1,
                "no single Key row reads {expected:?}; the rows read {:#?}",
                rows.iter()
                    .map(|(sentence, _)| sentence)
                    .collect::<Vec<_>>()
            );
            let chip = found[0].1;
            let chip_node = world.get::<Node>(chip).unwrap();
            assert_eq!(world.get::<BackgroundColor>(chip).unwrap().0, BOARD_WOOD);
            assert_eq!(chip_node.width, px(KEY_SWATCH_SIZE));
            assert_eq!(chip_node.justify_content, JustifyContent::Center);
            assert_eq!(chip_node.align_items, AlignItems::Center);
            // A sentence that wraps must not squeeze the chip beside it.
            assert_eq!(chip_node.flex_shrink, 0.0);
            world
                .get::<Children>(chip)
                .expect("a mark's chip holds its strokes")
                .iter()
                .map(|stroke| {
                    let node = world.get::<Node>(stroke).unwrap();
                    assert_eq!(node.position_type, PositionType::Absolute);
                    assert_eq!(node.width, node.height);
                    assert_eq!(
                        world.get::<BackgroundColor>(stroke).unwrap().0,
                        Color::NONE,
                        "a mark's stroke is not filled"
                    );
                    let (Val::Px(size), Val::Px(width)) = (node.width, node.border.left) else {
                        panic!("a mark's stroke is sized in pixels");
                    };
                    assert_eq!(node.border, UiRect::all(px(width)));
                    // An even margin either side, so the stroke centres
                    // on its chip to the pixel at the logical size as
                    // well as at twice it.
                    assert_eq!((KEY_SWATCH_SIZE - size) % 2.0, 0.0, "{size}");
                    (
                        world.get::<BorderColor>(stroke).unwrap().top,
                        size,
                        width,
                        node.border_radius,
                        world.get::<UiTransform>(stroke).unwrap().rotation,
                    )
                })
                .collect()
        };

        // Three strokes in the board's inks, outermost first: each starts
        // inside the one before it, every one of them shows two pixels,
        // and all three end at the same bare middle.
        let assert_three_strokes =
            |player: Player, mark: &[(Color, f32, f32, BorderRadius, Rot2)], inks: [Color; 3]| {
                assert_eq!(
                    mark.iter().map(|stroke| stroke.0).collect::<Vec<_>>(),
                    inks,
                    "{player:?}'s mark is painted outermost first, in the board's inks"
                );
                for pair in mark.windows(2) {
                    let showing = (pair[0].1 - pair[1].1) / 2.0;
                    assert!(showing >= 2.0, "{player:?}: a stroke under two pixels");
                }
                let middles: Vec<f32> = mark
                    .iter()
                    .map(|stroke| stroke.1 - 2.0 * stroke.2)
                    .collect();
                assert!(middles[0] >= 4.0, "{player:?}'s mark has no bare middle");
                assert_eq!(middles, [middles[0]; 3]);
                assert!(mark[2].2 >= 2.0);
            };

        let mut marker_spans = Vec::new();
        for (player, sentence) in [
            (
                Player::One,
                "Black / P1 Teleport Marker: a larger red diamond with a black border, shown \
                 only in Replays.",
            ),
            (
                Player::Two,
                "White / P2 Teleport Marker: a smaller blue diamond, white outside and dark \
                 inside, shown only in Replays.",
            ),
        ] {
            let diamond = strokes_beside(sentence);
            assert_three_strokes(player, &diamond, teleport_marker_strokes(player));
            for stroke in &diamond {
                assert_eq!(stroke.3, BorderRadius::ZERO);
                assert_eq!(
                    stroke.4,
                    Rot2::FRAC_PI_4,
                    "{player:?}'s swatch is not on its point"
                );
            }
            marker_spans.push(diamond[0].1 * std::f32::consts::SQRT_2);
        }
        assert!(
            marker_spans[0] <= KEY_SWATCH_SIZE,
            "the diamond overhangs its chip"
        );
        assert!(
            marker_spans[1] < marker_spans[0],
            "Player Two's is the smaller"
        );

        for (player, sentence) in [
            (
                Player::One,
                "Black / P1's hidden choices (up to three in Round 0): a hollow red ring, black \
                 outside and light inside.",
            ),
            (
                Player::Two,
                "White / P2's hidden choices (up to three in Round 0): a hollow blue ring, white \
                 outside and dark inside.",
            ),
        ] {
            let ring = strokes_beside(sentence);
            assert_three_strokes(player, &ring, hidden_selection_strokes(player));
            for stroke in &ring {
                assert_eq!(stroke.3, BorderRadius::all(percent(50)));
                assert_eq!(stroke.4, Rot2::IDENTITY);
            }
            assert!(ring[0].1 <= KEY_SWATCH_SIZE);
        }

        // The rest of the Key's copy that names a board visual, and its
        // title, are on the spawned modal too.
        let sentences: Vec<String> = world
            .query::<&Text>()
            .iter(&world)
            .map(|text| text.0.clone())
            .collect();
        for expected in [
            "Replay Key",
            "White / P2 cursor: blue with a white border.",
            "Black / P1 cursor: red with a black border.",
        ] {
            assert!(
                sentences.iter().any(|sentence| sentence == expected),
                "the Key does not read {expected:?}"
            );
        }
    }

    /// The Key's rows in the order they are drawn: White / P2 before
    /// Black / P1 for the cursors and for each Replay-only mark, then the
    /// three stones. The score badges' row follows the cursors, whose
    /// badges they are; it is one row for both players, and it says in
    /// words which side each badge is on.
    #[test]
    fn key_rows_run_white_before_black_for_every_mark() {
        let mut world = World::new();
        world
            .commands()
            .spawn(Node::default())
            .with_children(spawn_viewer_key_modal);
        world.flush();

        // The panel is the one node with more than one swatch-and-sentence
        // row among its children.
        let panels: Vec<Vec<String>> = world
            .query::<&Children>()
            .iter(&world)
            .map(|children| {
                children
                    .iter()
                    .filter_map(|row| {
                        let cells = world.get::<Children>(row)?;
                        if cells.len() != 2 {
                            return None;
                        }
                        Some(world.get::<Text>(cells[1])?.0.clone())
                    })
                    .collect::<Vec<_>>()
            })
            .filter(|rows| rows.len() > 1)
            .collect();
        assert_eq!(panels.len(), 1);
        let starts = [
            "White / P2 cursor",
            "Black / P1 cursor",
            "Score badges",
            "White / P2 Teleport Marker",
            "Black / P1 Teleport Marker",
            "White / P2's hidden choices",
            "Black / P1's hidden choices",
            "Black stones",
            "White stones",
            "Gray stones",
        ];
        assert_eq!(panels[0].len(), starts.len(), "{:#?}", panels[0]);
        for (row, start) in panels[0].iter().zip(starts) {
            assert!(row.starts_with(start), "{row:?} is not the {start:?} row");
        }
    }

    /// The Key's row for the score badges, read back from the spawned world
    /// and held to the board rather than to a constant: whichever badge
    /// [`score_badge_position`] puts on the left of a shared point, the
    /// sentence names that player as the left one and the swatch draws
    /// their frame first. So the board's order cannot change without the
    /// Key, which is the only place the game states it, changing with it.
    #[test]
    fn the_keys_badge_row_names_the_order_the_board_draws() {
        let shared = Vec2::ZERO;
        let left = if score_badge_position(shared, Player::One, true).x
            < score_badge_position(shared, Player::Two, true).x
        {
            Player::One
        } else {
            Player::Two
        };
        let right = match left {
            Player::One => Player::Two,
            Player::Two => Player::One,
        };
        let name = |player: Player| match player {
            Player::One => "Black / P1",
            Player::Two => "White / P2",
        };

        let mut world = World::new();
        world
            .commands()
            .spawn(Node::default())
            .with_children(spawn_viewer_key_modal);
        world.flush();

        let rows: Vec<(String, Entity)> = world
            .query::<&Children>()
            .iter(&world)
            .filter(|children| children.len() == 2)
            .filter_map(|children| {
                let sentence = world.get::<Text>(children[1])?.0.clone();
                Some((sentence, children[0]))
            })
            .filter(|(sentence, _)| sentence.starts_with("Score badges"))
            .collect();
        assert_eq!(rows.len(), 1, "the Key has one row for the score badges");
        let (sentence, chip) = &rows[0];
        let says = format!("{}'s is on the left", name(left));
        assert!(
            sentence.contains(&says),
            "{sentence:?} does not say {says:?}"
        );
        assert!(
            !sentence.contains(&format!("{}'s is on the left", name(right))),
            "{sentence:?}"
        );

        assert_eq!(world.get::<BackgroundColor>(*chip).unwrap().0, BOARD_WOOD);
        let chip_node = world.get::<Node>(*chip).unwrap();
        assert_eq!(chip_node.flex_direction, FlexDirection::Row);
        assert_eq!(chip_node.flex_shrink, 0.0);
        let badges: Vec<Entity> = world.get::<Children>(*chip).unwrap().to_vec();
        assert_eq!(badges.len(), 2, "two badges on the chip");
        for (badge, player) in badges.into_iter().zip([left, right]) {
            let node = world.get::<Node>(badge).unwrap();
            // In flow, so the first spawned is the one on the left.
            assert_eq!(node.position_type, PositionType::Relative);
            assert_eq!(node.border, UiRect::all(px(KEY_BADGE_FRAME)));
            assert_eq!(
                world.get::<BackgroundColor>(badge).unwrap().0,
                SCORE_BADGE_BACKGROUND
            );
            assert_eq!(
                *world.get::<BorderColor>(badge).unwrap(),
                BorderColor::all(player_accent(player)),
                "{player:?}'s badge is not where the board puts it"
            );
        }
        // The pair centres on its chip to the pixel, as every swatch does.
        let span = 2.0 * KEY_BADGE_SIZE.x + KEY_BADGE_GAP;
        assert!(span <= KEY_SWATCH_SIZE);
        assert_eq!((KEY_SWATCH_SIZE - span) % 2.0, 0.0);
        assert_eq!((KEY_SWATCH_SIZE - KEY_BADGE_SIZE.y) % 2.0, 0.0);
    }

    /// The Key replaced a legend that used to sit on the screen itself, and
    /// this holds that the legend's heading is gone from the Viewer's
    /// source. What the Key *does* say is asserted from the spawned modal,
    /// by the tests above: a scan for a sentence would find it in the
    /// assertion itself and could never fail.
    #[test]
    fn replay_viewer_has_no_persistent_legend() {
        // Every `.rs` file in this directory, read from disk at test time rather
        // than from a hand-written `include_str!` list - see
        // `crate::architecture` for why a list cannot guard a negative
        // assertion, and why the count is a floor the walk must reach.
        let source = sources_in("ui/replay/viewer", 5);
        let removed_legend = ["TELEPORT ", "MARKERS:"].concat();

        assert!(!source.contains(&removed_legend));
    }
}
