//! The shared colour and action-name vocabulary.
//!
//! Colours live in one place so screens stay visually consistent and so a
//! change is a single edit. Player identity in particular is defined once, by
//! [`player_accent`]: red for Black/Player 1, blue for White/Player 2. These
//! accents are used for cursors, headers, mapping columns and replay visuals
//! alike, so a player is recognisable everywhere.
//!
//! "One place" is a rule two tests hold, not a habit. No other file may
//! construct a colour or derive one - not a `Color::srgb` literal, and not a
//! translucent copy of a constant from here, which is why the translucent
//! panels below are constants of their own rather than a `with_alpha` call
//! at the screen that draws them
//! (`colours_are_constructed_only_in_presentation`, in `architecture.rs`).
//! And every colour made here must appear in the table of pairings in
//! `contrast.rs`, which checks each against the background it is drawn on
//! and the floor `CONTRIBUTING.md` sets. So adding a colour here fails
//! `cargo test` until that table says where it is used. That guard reads
//! this file's text, and knows a colour two ways: a line beginning
//! `pub(crate) const NAME: Color =`, and a top-level function whose return
//! type holds a `Color` and whose body builds one, as [`marimba_bar`] and
//! [`marimba_border`] do. A function that only chooses among the constants, like
//! [`player_accent`] or [`hidden_selection_strokes`], owes nothing more than
//! the constants do. Declare colours in one of those two shapes, and prefer
//! the constant.
//!
//! [`button_background`] folds a button's pointer [`Highlight`] (none,
//! hovered or pressed), an "active" flag and a "disabled" flag into a single
//! colour, and [`danger_button_background`] does the same for the highlight
//! alone, so no screen has to re-derive hover styling by hand.
//!
//! Alongside the colours, [`SELECT_ACTION`], [`SET_TELEPORT_MARKER_ACTION`]
//! and [`TELEPORT_TO_MARKER_ACTION`] are the player-facing names of the three
//! board actions that are neither movement nor restart.
//! `input::GameAction::label` and `input::controller::ProfileAction::label`
//! both read them, so the keyboard and controller mapping tabs cannot drift
//! apart on what an action is called. `ui::help` spells the two Teleport
//! terms out as literals in its own text instead - keep those in step if one
//! of these changes.

use bevy::prelude::*;

use crate::rules::Player;

pub(crate) const APP_BACKGROUND: Color = Color::srgb(0.055, 0.066, 0.08);
pub(crate) const PANEL_BACKGROUND: Color = Color::srgb(0.075, 0.09, 0.115);
pub(crate) const SURFACE_BACKGROUND: Color = Color::srgb(0.095, 0.115, 0.14);
pub(crate) const DEEP_SURFACE_BACKGROUND: Color = Color::srgb(0.065, 0.078, 0.096);
pub(crate) const OVERLAY_BACKGROUND: Color = Color::srgba(0.015, 0.02, 0.03, 0.92);
/// The Home screen's title panel: [`APP_BACKGROUND`], translucent enough to
/// let the backdrop diamonds show faintly through.
pub(crate) const HOME_PANEL_BACKGROUND: Color = Color::srgba(0.055, 0.066, 0.08, 0.96);
/// The Match screen's temporary control-hint cards: [`PANEL_BACKGROUND`],
/// slightly translucent.
pub(crate) const HINT_BACKGROUND: Color = Color::srgba(0.075, 0.09, 0.115, 0.94);
/// The Round 0 countdown card drawn over the live board:
/// [`PANEL_BACKGROUND`], slightly translucent.
pub(crate) const COUNTDOWN_BACKGROUND: Color = Color::srgba(0.075, 0.09, 0.115, 0.96);
/// The same card over the Replay Viewer's board, a little more translucent
/// so the recorded opening stays readable behind it.
pub(crate) const REPLAY_COUNTDOWN_BACKGROUND: Color = Color::srgba(0.075, 0.09, 0.115, 0.9);
/// The fill of a text field that is a panel of its own rather than a button:
/// the Replay Library's label editor.
pub(crate) const TEXT_FIELD_BACKGROUND: Color = Color::srgb(0.12, 0.145, 0.175);
/// The frame of a Replay Library card.
pub(crate) const CARD_BORDER: Color = Color::srgb(0.28, 0.34, 0.40);
/// The body of a Replay Viewer score badge, inside its player-accent frame.
pub(crate) const SCORE_BADGE_BACKGROUND: Color = Color::srgb(0.07, 0.085, 0.105);
pub(crate) const SCORE_BADGE_TEXT: Color = Color::srgb(0.96, 0.97, 0.98);

pub(crate) const ORANGE: Color = Color::srgb(0.95, 0.56, 0.24);
pub(crate) const ORANGE_ACTIVE: Color = Color::srgb(0.42, 0.27, 0.12);
pub(crate) const WHITE_P2_BLUE: Color = Color::srgb(0.16, 0.58, 1.0);
pub(crate) const BLACK_P1_RED: Color = Color::srgb(0.88, 0.12, 0.14);
pub(crate) const BLUE_BORDER: Color = Color::srgb(0.30, 0.56, 0.80);
pub(crate) const DANGER: Color = Color::srgb(1.0, 0.38, 0.32);
pub(crate) const DANGER_BACKGROUND: Color = Color::srgb(0.34, 0.10, 0.10);
pub(crate) const DANGER_HOVERED: Color = Color::srgb(0.42, 0.12, 0.12);
pub(crate) const DANGER_PRESSED: Color = Color::srgb(0.48, 0.08, 0.08);

pub(crate) const BUTTON_BACKGROUND: Color = Color::srgb(0.16, 0.20, 0.25);
pub(crate) const BUTTON_HOVERED: Color = Color::srgb(0.20, 0.31, 0.42);
pub(crate) const BUTTON_PRESSED: Color = Color::srgb(0.34, 0.25, 0.14);
pub(crate) const BUTTON_DISABLED: Color = Color::srgb(0.085, 0.10, 0.12);
pub(crate) const BUTTON_DISABLED_BORDER: Color = Color::srgb(0.20, 0.23, 0.26);

pub(crate) const TEXT_PRIMARY: Color = Color::srgb(0.94, 0.94, 0.91);
pub(crate) const TEXT_HEADING: Color = Color::srgb(0.98, 0.86, 0.66);
pub(crate) const TEXT_MUTED: Color = Color::srgb(0.68, 0.72, 0.76);
pub(crate) const TEXT_SUBTLE: Color = Color::srgb(0.48, 0.53, 0.58);

/// The board. Dark enough that a white stone, and the white border of
/// Player Two's cursor, stand 3:1 clear of it - which the wood it replaced,
/// `srgb(0.78, 0.55, 0.28)`, did not allow - and no darker, because every
/// step down takes contrast from the grid, the black stones and the gray
/// ones.
pub(crate) const BOARD_WOOD: Color = Color::srgb(0.705, 0.495, 0.25);
pub(crate) const BOARD_GRID: Color = Color::srgb(0.13, 0.105, 0.075);
pub(crate) const EMPTY_INTERSECTION: Color = Color::srgb(0.12, 0.095, 0.065);
pub(crate) const BLACK_STONE: Color = Color::srgb(0.025, 0.03, 0.04);
pub(crate) const WHITE_STONE: Color = Color::srgb(0.94, 0.95, 0.92);
pub(crate) const GRAY_STONE: Color = Color::srgb(0.38, 0.42, 0.46);
/// The rim of a white or gray stone - the grid's own ink, so the ring reads
/// as the lattice closing around the stone. `stone_geometry` in
/// `app::visuals` decides which stones carry it.
pub(crate) const STONE_RING: Color = BOARD_GRID;
/// The innermost stroke of Player One's hidden-selection mark in the Replay
/// Viewer: their red, almost white. It is what shows the mark on one of
/// their own black stones, where the mark's black outer stroke is lost; see
/// [`hidden_selection_strokes`].
pub(crate) const BLACK_P1_PALE_RED: Color = Color::srgb(1.0, 0.86, 0.84);
/// The innermost stroke of Player Two's hidden-selection mark and of their
/// Teleport Marker: their blue, almost black, for the white stone that
/// swallows either mark's white outer stroke.
pub(crate) const WHITE_P2_DEEP_BLUE: Color = Color::srgb(0.02, 0.045, 0.11);
/// An empty cell of a Replay Library thumbnail: wood, darker than
/// [`BOARD_WOOD`] so the thumbnail's frame shows around the grid.
pub(crate) const THUMBNAIL_EMPTY: Color = Color::srgb(0.60, 0.41, 0.22);
/// The faint line between two thumbnail cells that carry no ring.
pub(crate) const THUMBNAIL_GRID: Color = Color::srgba(0.1, 0.08, 0.05, 0.45);

/// A bar of the Home screen's marimba at rest, and at the instant it is
/// struck; [`marimba_bar`] fades between the two.
pub(crate) const MARIMBA_BAR: Color = Color::srgb(0.42, 0.23, 0.09);
pub(crate) const MARIMBA_BAR_STRUCK: Color = Color::srgb(0.98, 0.78, 0.56);
pub(crate) const MARIMBA_BORDER: Color = Color::srgba(0.16, 0.09, 0.035, 0.9);

pub(crate) const SELECT_ACTION: &str = "Select / Revise Selection";
pub(crate) const SET_TELEPORT_MARKER_ACTION: &str = "Set Teleport Marker";
pub(crate) const TELEPORT_TO_MARKER_ACTION: &str = "Teleport to Marker";

pub(crate) const fn player_accent(player: Player) -> Color {
    match player {
        Player::One => BLACK_P1_RED,
        Player::Two => WHITE_P2_BLUE,
    }
}

/// The three strokes of a player's hidden selection as the Replay Viewer
/// reveals it, outermost first: a ring with nothing in the middle, so the
/// wood or the player's own stone beneath shows through. The live board
/// never draws one.
///
/// The outer strokes are the two stone colours, opposed black against
/// white: that is what tells the players' marks apart without hue - one is
/// dark outside and light inside, the other the reverse - exactly as the
/// cursors' borders do. It is also why there are three strokes. A player
/// may select one of their own stones, which is the outer stroke's colour,
/// so the innermost stroke is the one that carries the mark there. And a
/// hand-edited Replay can put a mark on any stone, where no single ink
/// could carry it - none is 3:1 clear of the wood and all three stones at
/// once - but the outer and innermost strokes between them do. The accent
/// between those two keeps each mark inside its player's hue family, as
/// the two tones made for the innermost strokes do, so nothing has to be
/// decoded across players. `contrast.rs` holds every one of those ratios.
pub(crate) const fn hidden_selection_strokes(player: Player) -> [Color; 3] {
    match player {
        Player::One => [BLACK_STONE, BLACK_P1_RED, BLACK_P1_PALE_RED],
        Player::Two => [WHITE_STONE, WHITE_P2_BLUE, WHITE_P2_DEEP_BLUE],
    }
}

/// The three strokes of a player's Teleport Marker in the Replay Viewer,
/// outermost first. The live board never draws one.
///
/// The outer stroke is the player's stone colour and the accent lies
/// inside it, as in [`hidden_selection_strokes`] and for the same reasons:
/// opposed outer strokes tell the two players' marks apart without hue,
/// and the accent keeps each in its player's hue family. The innermost
/// stroke is whatever carries the marker on a stone of the outer stroke's
/// own colour, where that stroke is lost - and a marker, unlike a hidden
/// selection, is set where the cursor stands, so it lies on any stone. For
/// Player Two that has to be their deep blue: their accent alone is 2.75:1
/// on a white stone, the deep blue 17.35:1. For Player One the accent
/// already carries it, red on a black stone at 4.21:1, and the innermost
/// stroke stays black.
///
/// That is the one place a player's two marks differ - the ring follows
/// the owner's rule for both players, stone colour outside, accent, then
/// the player's pale or deep tone inside, and the marker follows it for
/// Player Two only - and it was decided from captures rather than for
/// symmetry. Three strokes fit a side of 40 easily; the trouble was what
/// the third lay against. Player One's marker is the larger, so it is the
/// outside of every pair of marks it shares a point with: Player Two's
/// marker nests inside it with three pixels of board between, and when
/// Player One marks a point Player Two selects, Player Two's ring sits
/// inside it with its white outer stroke against the diamond. A pale
/// innermost stroke put a light edge against a light edge along every
/// side in both, and softened the marker on the wood for nothing Player
/// One needed; a black one keeps the dark edge that separates them. (Player
/// One's ring keeps its pale stroke over Player Two's marker, where it
/// crosses the diamond's tips rather than running the length of its sides,
/// and the captures bore that.) Player Two's deep blue costs nothing of
/// the kind: nothing of Player One's lies inside Player Two's marker except
/// in Round 0, when both players chose the point and the smaller rings
/// drawn for it cross the diamond, which is the "two rings over a broken
/// diamond" case `docs/ARCHITECTURE.md` accepts.
pub(crate) const fn teleport_marker_strokes(player: Player) -> [Color; 3] {
    match player {
        Player::One => [BLACK_STONE, BLACK_P1_RED, BLACK_STONE],
        Player::Two => [WHITE_STONE, WHITE_P2_BLUE, WHITE_P2_DEEP_BLUE],
    }
}

/// One bar of the Home marimba: [`MARIMBA_BAR`] at `highlight` 0, brightening
/// to [`MARIMBA_BAR_STRUCK`] at 1, at the instrument's current fade `alpha`.
pub(crate) fn marimba_bar(alpha: f32, highlight: f32) -> Color {
    let highlight = highlight.clamp(0.0, 1.0);
    let rest = MARIMBA_BAR.to_srgba();
    let struck = MARIMBA_BAR_STRUCK.to_srgba();
    // Weighted, not offset, so both ends of the fade are the named colours
    // exactly and the contrast table can hold the function to them.
    let fade = |rest: f32, struck: f32| rest * (1.0 - highlight) + struck * highlight;
    Color::srgba(
        fade(rest.red, struck.red),
        fade(rest.green, struck.green),
        fade(rest.blue, struck.blue),
        alpha,
    )
}

/// The frame of a marimba bar at the instrument's current fade `alpha`.
pub(crate) fn marimba_border(alpha: f32) -> Color {
    let border = MARIMBA_BORDER.to_srgba();
    border.with_alpha(border.alpha * alpha).into()
}

/// The fill of a captured stone as the capture animation fades it toward
/// its captor's colour: the `owner`'s stone colour at `fade` 0, the
/// opponent's at 1, at `alpha`. The captor is always the opponent - the
/// rules credit every removal to the removed stone's opponent - so the
/// owner alone picks the two ends. Weighted, not offset, like
/// [`marimba_bar`], so both ends are the named stone colours exactly and
/// the contrast table can hold the function to them; what the fade passes
/// through on the way is `ui::match_screen::capture`'s business and is
/// discussed there.
pub(crate) fn captured_stone_fill(owner: Player, fade: f32, alpha: f32) -> Color {
    let fade = fade.clamp(0.0, 1.0);
    let (from, to) = match owner {
        Player::One => (BLACK_STONE, WHITE_STONE),
        Player::Two => (WHITE_STONE, BLACK_STONE),
    };
    let (from, to) = (from.to_srgba(), to.to_srgba());
    let blend = |from: f32, to: f32| from * (1.0 - fade) + to * fade;
    Color::srgba(
        blend(from.red, to.red),
        blend(from.green, to.green),
        blend(from.blue, to.blue),
        alpha,
    )
}

/// The fill of one of `player`'s Round 0 dots as it crosses its row's
/// dividing line: an unfilled dot's [`DEEP_SURFACE_BACKGROUND`] at `fill`
/// 0, the player's accent at 1, at `alpha`. Weighted like
/// [`captured_stone_fill`], so both ends are the named colours exactly and
/// the contrast table can hold the function to them; the tones between
/// are on screen only while a dot is moving, which
/// `ui::match_screen::opening_dots` discusses.
pub(crate) fn opening_dot_fill(player: Player, fill: f32, alpha: f32) -> Color {
    let fill = fill.clamp(0.0, 1.0);
    let (from, to) = (
        DEEP_SURFACE_BACKGROUND.to_srgba(),
        player_accent(player).to_srgba(),
    );
    let blend = |from: f32, to: f32| from * (1.0 - fill) + to * fill;
    Color::srgba(
        blend(from.red, to.red),
        blend(from.green, to.green),
        blend(from.blue, to.blue),
        alpha,
    )
}

/// The frame of one of `player`'s Round 0 dots - their accent - at
/// `alpha`, for a dot fading into or out of its row.
pub(crate) fn opening_dot_frame(player: Player, alpha: f32) -> Color {
    player_accent(player).with_alpha(alpha)
}

/// Which of its three pointer fills a button wears: what the pointer is
/// doing to it, and nothing else - the keyboard and controller ring draws an
/// outline instead (`ui::focus`).
///
/// Read from a button's `Pressed` and `Hovered` components by
/// `ui::button`'s `PointerHighlight`. A press outranks the hover and lasts
/// for as long as the button is held, even after the pointer has slid off
/// it. Releasing there does not activate the button, so a fill that went
/// back to normal would say so sooner; but `Hovered` follows the mouse
/// alone, so a pressed fill that also waited for the hover would never show
/// a touch's press at all.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) enum Highlight {
    #[default]
    None,
    Hovered,
    Pressed,
}

impl Highlight {
    pub(crate) const fn new(pressed: bool, hovered: bool) -> Self {
        if pressed {
            Self::Pressed
        } else if hovered {
            Self::Hovered
        } else {
            Self::None
        }
    }
}

pub(crate) fn button_background(highlight: Highlight, active: bool, disabled: bool) -> Color {
    if disabled {
        BUTTON_DISABLED
    } else if active {
        ORANGE_ACTIVE
    } else {
        match highlight {
            Highlight::Pressed => BUTTON_PRESSED,
            Highlight::Hovered => BUTTON_HOVERED,
            Highlight::None => BUTTON_BACKGROUND,
        }
    }
}

pub(crate) fn danger_button_background(highlight: Highlight) -> Color {
    match highlight {
        Highlight::Pressed => DANGER_PRESSED,
        Highlight::Hovered => DANGER_HOVERED,
        Highlight::None => DANGER_BACKGROUND,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_palette_maps_white_p2_to_blue_and_black_p1_to_red() {
        assert_eq!(ORANGE, Color::srgb(0.95, 0.56, 0.24));
        assert_eq!(WHITE_P2_BLUE, Color::srgb(0.16, 0.58, 1.0));
        assert_eq!(player_accent(Player::Two), WHITE_P2_BLUE);
        assert_eq!(player_accent(Player::One), BLACK_P1_RED);
        assert_eq!(BLACK_P1_RED, Color::srgb(0.88, 0.12, 0.14));
        assert_ne!(WHITE_P2_BLUE, DANGER);
        assert_ne!(BLACK_P1_RED, DANGER);
        assert_eq!(BLACK_STONE, Color::srgb(0.025, 0.03, 0.04));
        assert_eq!(WHITE_STONE, Color::srgb(0.94, 0.95, 0.92));
        assert_eq!(GRAY_STONE, Color::srgb(0.38, 0.42, 0.46));
    }

    /// A translucent constant spells out the channels of the colour it is a
    /// copy of, because `with_alpha` is not `const`. This is what keeps the
    /// copy from drifting when the original is retuned.
    #[test]
    fn translucent_constants_are_copies_of_the_colours_they_name() {
        assert_eq!(HOME_PANEL_BACKGROUND, APP_BACKGROUND.with_alpha(0.96));
        assert_eq!(HINT_BACKGROUND, PANEL_BACKGROUND.with_alpha(0.94));
        assert_eq!(COUNTDOWN_BACKGROUND, PANEL_BACKGROUND.with_alpha(0.96));
        assert_eq!(
            REPLAY_COUNTDOWN_BACKGROUND,
            PANEL_BACKGROUND.with_alpha(0.9)
        );
        assert_eq!(STONE_RING, BOARD_GRID);
    }

    /// The two properties of a hidden-selection mark that are choices
    /// rather than arithmetic: the accent is its middle stroke, and the
    /// two players' marks are opposed end to end - Player One's is the
    /// black stone's colour outside, Player Two's the white stone's - with
    /// every stroke opaque, since opacity is what the marks they replaced
    /// relied on and what vanished over a stone of their own colour.
    /// `contrast.rs` holds the ratios.
    #[test]
    fn hidden_selection_strokes_are_opaque_and_opposed_outside() {
        let [one_outer, one_ring, one_inner] = hidden_selection_strokes(Player::One);
        let [two_outer, two_ring, two_inner] = hidden_selection_strokes(Player::Two);
        assert_eq!((one_outer, two_outer), (BLACK_STONE, WHITE_STONE));
        assert_eq!(one_ring, player_accent(Player::One));
        assert_eq!(two_ring, player_accent(Player::Two));
        assert_eq!(one_inner, BLACK_P1_PALE_RED);
        assert_eq!(two_inner, WHITE_P2_DEEP_BLUE);
        for stroke in [
            one_outer, one_ring, one_inner, two_outer, two_ring, two_inner,
        ] {
            assert_eq!(stroke.to_srgba().alpha, 1.0);
        }
    }

    /// A Teleport Marker runs the way its player's hidden-selection mark
    /// does, outside and in the middle, and inside it carries what that
    /// player needs on a stone of their own colour and no more: Player
    /// Two's deep blue, because their accent is below the floor there,
    /// and for Player One the black again, because their accent is not
    /// and the pale red cost the nested marks their dark edge. The doc
    /// comment on `teleport_marker_strokes` has the captures' verdict;
    /// `contrast.rs` holds the ratios.
    #[test]
    fn teleport_marker_strokes_match_the_ring_but_for_player_ones_inside() {
        for player in Player::ALL {
            let [outer, accent, _] = teleport_marker_strokes(player);
            let [ring_outer, ring_accent, _] = hidden_selection_strokes(player);
            assert_eq!((outer, accent), (ring_outer, ring_accent), "{player:?}");
            for stroke in teleport_marker_strokes(player) {
                assert_eq!(stroke.to_srgba().alpha, 1.0);
            }
        }
        let [.., one_inner] = teleport_marker_strokes(Player::One);
        let [.., two_inner] = teleport_marker_strokes(Player::Two);
        assert_eq!(one_inner, BLACK_STONE);
        assert_eq!(two_inner, WHITE_P2_DEEP_BLUE);
    }

    /// The owner's requirement of the two tones made for the innermost
    /// strokes: each stays inside its own player's hue family - a red,
    /// however pale, and a blue, however deep - so that no stroke of one
    /// player's mark could be taken for the other's.
    #[test]
    fn hidden_selection_inner_strokes_stay_in_their_players_hue_family() {
        let pale = BLACK_P1_PALE_RED.to_srgba();
        assert!(pale.red > pale.green && pale.red > pale.blue);
        let deep = WHITE_P2_DEEP_BLUE.to_srgba();
        assert!(deep.blue > deep.red && deep.blue > deep.green);
    }

    #[test]
    fn captured_stones_fade_between_the_two_named_stone_colours() {
        assert_eq!(captured_stone_fill(Player::One, 0.0, 1.0), BLACK_STONE);
        assert_eq!(captured_stone_fill(Player::One, 1.0, 1.0), WHITE_STONE);
        assert_eq!(captured_stone_fill(Player::Two, 0.0, 1.0), WHITE_STONE);
        assert_eq!(captured_stone_fill(Player::Two, 1.0, 1.0), BLACK_STONE);
        assert_eq!(
            captured_stone_fill(Player::One, -1.0, 1.0),
            BLACK_STONE,
            "fade is clamped"
        );
        assert_eq!(
            captured_stone_fill(Player::Two, 2.0, 1.0),
            BLACK_STONE,
            "fade is clamped"
        );
        assert_eq!(
            captured_stone_fill(Player::One, 0.0, 0.25),
            BLACK_STONE.with_alpha(0.25)
        );
        // Halfway is halfway in each channel: the two directions meet.
        assert_eq!(
            captured_stone_fill(Player::One, 0.5, 1.0),
            captured_stone_fill(Player::Two, 0.5, 1.0)
        );
    }

    #[test]
    fn opening_dots_fill_between_the_unfilled_dot_and_their_players_accent() {
        for player in Player::ALL {
            assert_eq!(opening_dot_fill(player, 0.0, 1.0), DEEP_SURFACE_BACKGROUND);
            assert_eq!(opening_dot_fill(player, 1.0, 1.0), player_accent(player));
            assert_eq!(
                opening_dot_fill(player, 2.0, 1.0),
                player_accent(player),
                "fill is clamped"
            );
            assert_eq!(
                opening_dot_fill(player, 0.0, 0.5),
                DEEP_SURFACE_BACKGROUND.with_alpha(0.5)
            );
            assert_eq!(opening_dot_frame(player, 1.0), player_accent(player));
            assert_eq!(
                opening_dot_frame(player, 0.25),
                player_accent(player).with_alpha(0.25)
            );
        }
    }

    #[test]
    fn marimba_bars_fade_between_their_two_named_colours() {
        assert_eq!(marimba_bar(1.0, 0.0), MARIMBA_BAR);
        assert_eq!(marimba_bar(1.0, -3.0), MARIMBA_BAR, "highlight is clamped");
        assert_eq!(marimba_bar(1.0, 1.0), MARIMBA_BAR_STRUCK);
        assert_eq!(
            marimba_bar(1.0, 7.0),
            MARIMBA_BAR_STRUCK,
            "highlight is clamped"
        );
        assert_eq!(marimba_bar(0.25, 0.0), MARIMBA_BAR.with_alpha(0.25));
        assert_eq!(marimba_border(1.0), MARIMBA_BORDER);
        assert_eq!(marimba_border(0.0).to_srgba().alpha, 0.0);
    }

    #[test]
    fn shared_button_states_preserve_neutral_and_accent_roles() {
        assert_eq!(
            button_background(Highlight::None, false, false),
            BUTTON_BACKGROUND
        );
        assert_eq!(
            button_background(Highlight::Hovered, false, false),
            BUTTON_HOVERED
        );
        assert_eq!(
            button_background(Highlight::None, true, false),
            ORANGE_ACTIVE
        );
        assert_eq!(
            button_background(Highlight::Pressed, true, true),
            BUTTON_DISABLED
        );
    }

    #[test]
    fn primary_action_terms_are_centralized() {
        assert_eq!(SELECT_ACTION, "Select / Revise Selection");
        assert_eq!(SET_TELEPORT_MARKER_ACTION, "Set Teleport Marker");
        assert_eq!(TELEPORT_TO_MARKER_ACTION, "Teleport to Marker");
    }
}
