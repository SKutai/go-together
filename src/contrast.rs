//! Test-only guard that holds the game's colours to the contrast floor
//! `CONTRIBUTING.md` states under "UI and accessibility".
//!
//! # What it checks
//!
//! The floor is WCAG 2.1 AA: 4.5:1 for text, 3:1 for text of 24px and
//! larger, and 3:1 for a non-text mark that carries meaning. A colour is
//! only wrong against the background it is really drawn on, so the floor is
//! checked over a table of pairings - [`pairs`] - rather than over every
//! combination of two constants. Each row names a foreground, the background
//! it sits on or the colour it has to be told apart from, what it owes that
//! background, and where on screen the pairing occurs.
//!
//! A pairing that cannot meet the floor is not left out. It goes in
//! [`exemptions`] with the reason it falls short and one of three answers to
//! "then how does the player get the meaning": a second, non-colour channel
//! that carries it; a statement that WCAG itself asks nothing of it, because
//! it is decoration or part of a control that is switched off; or the plain
//! admission that nothing carries it yet and the owner has to decide. That
//! last kind is counted, in [`AWAITING_OWNER_DECISION`], so the debt can
//! shrink but cannot grow without someone editing a number on purpose. The
//! count has been zero since 2026-10-04, and the kind is kept for the next
//! pairing that needs it.
//!
//! # What keeps the table honest
//!
//! A hand-written table rots in three ways, and each has a test:
//!
//! * **A colour nobody listed.** `every_colour_constant_is_in_the_table`
//!   reads `presentation.rs` off disk, the way the guards in
//!   [`crate::architecture`] read the crate, and requires every `Color`
//!   constant declared there to appear in at least one row;
//!   `every_colour_made_by_a_function_is_in_the_table` requires the same of
//!   every function there that builds a colour rather than choosing one. A
//!   new colour cannot be added without saying where it is drawn. That
//!   would be an empty promise if a colour could be written inline in a
//!   screen instead, so `architecture` holds the other half: no colour is
//!   constructed outside `presentation.rs` at all. What that scan cannot
//!   see - a default, a renamed type - is listed on
//!   `colour_constructions` there.
//! * **An exemption that outlived its reason.**
//!   `every_exemption_is_still_needed` fails on an exempt pairing that now
//!   meets the floor, exactly as loudly as `every_declared_pairing_meets_the_floor`
//!   fails on a declared one that does not. Fixing a colour therefore means
//!   moving its row, which is the point: the list of exemptions is a
//!   document of what is still wrong, and it must stay true.
//! * **Arithmetic that drifted.** `the_arithmetic_matches_known_values`
//!   pins the luminance and ratio formulas to published values, and the
//!   compositing to a pixel measured from the running game.
//!
//! What no test can hold is whether a row's *place* is true - whether that
//! foreground really is drawn on that background. The rows were built from
//! an inventory of every draw site in `src/ui/` and `src/app/visuals.rs`,
//! taken on 2026-09-30, and the Replay Viewer's two board marks were
//! inventoried again on 2026-10-01 when they changed shape and on
//! 2026-10-02 when the Teleport Marker took a third stroke; a change to how
//! a screen nests its panels is a reason to re-read the rows that name it.
//!
//! # Translucent colours
//!
//! A translucent colour has no contrast of its own, only that of what it
//! becomes over whatever is beneath it. [`Ink::over`] composites the two in
//! linear light, which is how the renderer blends both `bevy_ui` nodes and
//! world-space sprites onto its sRGB target; the measured pixel in
//! `the_arithmetic_matches_known_values` is the evidence that it does.
//!
//! "Beneath it" needs care for a border. `bevy_ui` does not paint a node's
//! own fill under its border, so a translucent border is composited over
//! whatever lies behind the *node* - the thumbnail's wood under a cell's
//! faint grid line, not the cell's fill. That was also read off a capture:
//! the grid lines darkened with the board when the board was darkened.
//!
//! # Adding a colour
//!
//! Declare it in `presentation.rs`, then add a row here for each background
//! it is drawn on: [`text`] with the *smallest* font size the pairing is
//! used at, or [`graphic`] for a border, a fill or a shape. If a row fails,
//! change the colour; if the colour cannot change, the row belongs in
//! [`exemptions`] with a reason a reviewer can check.

use bevy::prelude::*;

use crate::architecture::{colour_constructions, source_file};
use crate::presentation::*;
use crate::rules::Player;

/// Text below this size owes its background 4.5:1; at or above it, 3:1.
/// WCAG also counts bold text from 18.66px as large, but the game sets
/// every label in one weight, so size is the only way a row qualifies.
const LARGE_TEXT_PX: f32 = 24.0;

/// How many pairings are below the floor with nothing else carrying their
/// meaning. Each is an exemption marked [`Excuse::AwaitingOwnerDecision`],
/// and `docs/ideas/accessibility-findings.md` records what the decision is
/// about. Lower this when one is resolved; raising it is a decision to ship
/// something a player may not be able to see, and should be argued as one.
///
/// It is zero, and pinned at zero on purpose: that is the assertion that
/// the game has no contrast debt today, and it fails the moment a row is
/// added as awaiting a decision rather than fixed. It reached zero on
/// 2026-10-04, when the last six were closed.
const AWAITING_OWNER_DECISION: usize = 0;

/// The WCAG relative luminance of an opaque colour.
fn luminance(color: Color) -> f32 {
    let linear = color.to_linear();
    0.2126 * linear.red + 0.7152 * linear.green + 0.0722 * linear.blue
}

/// The WCAG contrast ratio of two opaque colours, from 1.0 to 21.0.
fn contrast(first: Color, second: Color) -> f32 {
    let (first, second) = (luminance(first), luminance(second));
    (first.max(second) + 0.05) / (first.min(second) + 0.05)
}

/// A colour as the table names it: the expression it was written as, kept
/// so a failure can say which constant to look at and so the completeness
/// guard can tell which constants the table covers.
#[derive(Clone, Debug)]
struct Ink {
    name: String,
    color: Color,
}

macro_rules! ink {
    ($color:expr) => {
        Ink {
            name: stringify!($color).replace(' ', ""),
            color: $color,
        }
    };
    // A constant together with the function call that draws it. The two
    // must be the same colour, and the row then names both - which is how a
    // function that makes a colour gets into the table.
    ($constant:expr, $drawn:expr) => {{
        assert_eq!(
            $constant,
            $drawn,
            "{} is not what {} draws",
            stringify!($constant),
            stringify!($drawn)
        );
        Ink {
            name: format!(
                "{}={}",
                stringify!($constant),
                stringify!($drawn).replace(' ', "")
            ),
            color: $constant,
        }
    }};
}

impl Ink {
    /// This colour as it appears over `below`: source-over compositing by
    /// this colour's alpha, in linear light. `below` must be opaque.
    fn over(self, below: Ink) -> Ink {
        let (top, bottom) = (self.color.to_linear(), below.color.to_linear());
        assert_eq!(
            bottom.alpha, 1.0,
            "{} is translucent; composite it over something opaque first",
            below.name
        );
        let mix = |top_channel: f32, bottom_channel: f32| {
            top_channel * top.alpha + bottom_channel * (1.0 - top.alpha)
        };
        Ink {
            name: format!("{} over {}", self.name, below.name),
            color: Color::linear_rgb(
                mix(top.red, bottom.red),
                mix(top.green, bottom.green),
                mix(top.blue, bottom.blue),
            ),
        }
    }
}

fn top_alpha(color: Color) -> f32 {
    color.to_linear().alpha
}

/// What a foreground owes the colour it is paired with.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Need {
    /// Text, at the smallest size this pairing is drawn at.
    Text(f32),
    /// A mark that is not text and carries meaning: a control's border, a
    /// stone's edge, a bar's fill, or two fills the player must tell apart.
    Graphic,
}

impl Need {
    fn floor(self) -> f32 {
        match self {
            Need::Text(px) if px < LARGE_TEXT_PX => 4.5,
            Need::Text(_) | Need::Graphic => 3.0,
        }
    }
}

/// One pairing that occurs on screen.
#[derive(Clone, Debug)]
struct Pair {
    foreground: Ink,
    background: Ink,
    need: Need,
    place: &'static str,
}

impl Pair {
    fn ratio(&self) -> f32 {
        for side in [&self.foreground, &self.background] {
            assert_eq!(
                top_alpha(side.color),
                1.0,
                "{} is translucent and has no contrast of its own; composite it with `over`",
                side.name
            );
        }
        contrast(self.foreground.color, self.background.color)
    }

    fn meets_the_floor(&self) -> bool {
        self.ratio() >= self.need.floor()
    }

    fn describe(&self) -> String {
        format!(
            "{} on {}: {:.2}:1, needs {:.1}:1 - {}",
            self.foreground.name,
            self.background.name,
            self.ratio(),
            self.need.floor(),
            self.place
        )
    }
}

fn text(foreground: Ink, background: Ink, smallest_px: f32, place: &'static str) -> Pair {
    Pair {
        foreground,
        background,
        need: Need::Text(smallest_px),
        place,
    }
}

fn graphic(foreground: Ink, background: Ink, place: &'static str) -> Pair {
    Pair {
        foreground,
        background,
        need: Need::Graphic,
        place,
    }
}

/// Why a pairing below the floor is allowed to stand.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Excuse {
    /// The meaning also reaches the player by this channel, which is not
    /// colour.
    SecondChannel(&'static str),
    /// WCAG asks nothing of this pairing: it is decoration, or part of a
    /// control that is switched off. The text says which.
    NotRequired(&'static str),
    /// Nothing else carries the meaning, and what to do about it is the
    /// owner's decision rather than a contributor's. Counted in
    /// [`AWAITING_OWNER_DECISION`].
    ///
    /// No row in [`exemptions`] uses this today, and that is deliberate
    /// rather than dead code: the variant is kept so that the next pairing
    /// which cannot be fixed on the spot has an honest place to go, and so
    /// that adding one is visible as a change to a count pinned at zero.
    /// `a_pairing_awaiting_a_decision_would_be_counted` constructs it, to
    /// show that the count would see such a row - which is what makes the
    /// pin at zero an assertion - and that is also why the compiler does
    /// not report it as never constructed, with no `expect` needed.
    AwaitingOwnerDecision,
}

/// A pairing that is below the floor, and the account of it.
#[derive(Clone, Debug)]
struct Exemption {
    pair: Pair,
    /// Why the pairing falls short and cannot simply be recoloured.
    reason: &'static str,
    excuse: Excuse,
}

fn exempt(pair: Pair, reason: &'static str, excuse: Excuse) -> Exemption {
    Exemption {
        pair,
        reason,
        excuse,
    }
}

/// Every pairing that occurs on screen and meets the floor.
fn pairs() -> Vec<Pair> {
    let home_over_panel = || ink!(HOME_PANEL_BACKGROUND).over(ink!(PANEL_BACKGROUND));
    let home_over_orange = || ink!(HOME_PANEL_BACKGROUND).over(ink!(ORANGE));
    let home_over_app = || ink!(HOME_PANEL_BACKGROUND).over(ink!(APP_BACKGROUND));
    let hint = || ink!(HINT_BACKGROUND).over(ink!(APP_BACKGROUND));
    let scrim = || ink!(OVERLAY_BACKGROUND).over(ink!(APP_BACKGROUND));
    // The lightest thing a scrim or a countdown card can be drawn over.
    let scrim_over_white = || ink!(OVERLAY_BACKGROUND).over(ink!(WHITE_STONE));

    vec![
        // ---- Text on the window background --------------------------------
        text(
            ink!(TEXT_HEADING),
            ink!(APP_BACKGROUND),
            16.0,
            "Match and Replay Viewer titles, timers and round lines; the Replay Library title",
        ),
        text(
            ink!(TEXT_MUTED),
            ink!(APP_BACKGROUND),
            14.0,
            "the Match end-gesture hint and end-reason line, the Viewer result line, the Library's \
             retention and page lines",
        ),
        text(
            ink!(DANGER),
            ink!(APP_BACKGROUND),
            14.0,
            "the Replay Library's storage warning",
        ),
        // The two players' headings are 24px so that their accents are held
        // to the large-text floor, which Player One's red passes and 4.5:1 it
        // does not. A redder red could reach 4.5:1 on this background, but
        // not on SURFACE_BACKGROUND, where the same accent heads the Settings
        // columns (`docs/ideas/accessibility-findings.md` has the proof), so
        // 24px was the owner's answer for both, on 2026-10-03.
        text(
            ink!(BLACK_P1_RED),
            ink!(APP_BACKGROUND),
            24.0,
            "Player One's sidebar heading on the Match screen",
        ),
        text(
            ink!(WHITE_P2_BLUE),
            ink!(APP_BACKGROUND),
            24.0,
            "Player Two's sidebar heading on the Match screen",
        ),
        // ---- Text on the Home panel ---------------------------------------
        // The panel is translucent over two backdrop diamonds, so it is three
        // different backgrounds: over the inner diamond, over the orange band
        // around it, and over the window beyond both.
        text(
            ink!(TEXT_HEADING),
            home_over_panel(),
            68.0,
            "the Home title, over the inner diamond",
        ),
        text(
            ink!(TEXT_HEADING),
            home_over_orange(),
            68.0,
            "the Home title, where the orange band passes beneath it",
        ),
        text(
            ink!(TEXT_HEADING),
            home_over_app(),
            68.0,
            "the Home title, beyond both diamonds",
        ),
        text(
            ink!(TEXT_MUTED),
            home_over_panel(),
            14.0,
            "the Home tagline and footer line, over the inner diamond",
        ),
        text(
            ink!(TEXT_MUTED),
            home_over_orange(),
            14.0,
            "the few glyphs of the Home footer line that cross the orange band",
        ),
        text(
            ink!(TEXT_MUTED),
            home_over_app(),
            14.0,
            "the ends of the Home footer line, beyond both diamonds",
        ),
        // ---- Text on panels and surfaces ----------------------------------
        text(
            ink!(TEXT_HEADING),
            ink!(PANEL_BACKGROUND),
            16.0,
            "modal and Settings titles; Match Settings values in the Viewer",
        ),
        text(
            ink!(TEXT_PRIMARY),
            ink!(PANEL_BACKGROUND),
            16.0,
            "Help, Key, Match Settings and confirmation body text",
        ),
        text(
            ink!(TEXT_MUTED),
            ink!(PANEL_BACKGROUND),
            14.0,
            "Help page line, Settings instruction, dialog hints, the Match ID in the delete dialog",
        ),
        text(
            ink!(DANGER),
            ink!(PANEL_BACKGROUND),
            14.0,
            "Settings and dialog error lines; the delete dialog's title",
        ),
        text(
            ink!(TEXT_PRIMARY),
            ink!(SURFACE_BACKGROUND),
            17.0,
            "Settings row labels; Replay Library card labels",
        ),
        text(
            ink!(TEXT_HEADING),
            ink!(SURFACE_BACKGROUND),
            24.0,
            "the SHARED column heading on the Settings Keyboard tab",
        ),
        text(
            ink!(TEXT_MUTED),
            ink!(SURFACE_BACKGROUND),
            14.0,
            "Settings descriptions and units; Replay Library card summaries",
        ),
        text(
            ink!(TEXT_SUBTLE),
            ink!(SURFACE_BACKGROUND),
            14.0,
            "the Match ID line on a Replay Library card",
        ),
        text(
            ink!(DANGER),
            ink!(SURFACE_BACKGROUND),
            14.0,
            "a Settings Game-tab field's error line",
        ),
        text(
            ink!(BLACK_P1_RED),
            ink!(SURFACE_BACKGROUND),
            24.0,
            "Player One's column heading on the Settings binding tabs",
        ),
        text(
            ink!(WHITE_P2_BLUE),
            ink!(SURFACE_BACKGROUND),
            24.0,
            "Player Two's column heading on the Settings binding tabs",
        ),
        // The hint's text is not in its player's accent, which at 14px is
        // below 4.5:1 on the card; the card's frame, its side of the screen
        // and its first line, which names the player, say whose it is.
        text(
            ink!(TEXT_MUTED),
            hint(),
            14.0,
            "both players' control hints on the Match screen",
        ),
        text(
            ink!(TEXT_MUTED),
            ink!(DEEP_SURFACE_BACKGROUND),
            22.0,
            "the Replay Library's empty-list message",
        ),
        text(
            ink!(TEXT_PRIMARY),
            ink!(TEXT_FIELD_BACKGROUND),
            19.0,
            "the Replay label being edited in the rename dialog",
        ),
        text(
            ink!(SCORE_BADGE_TEXT),
            ink!(SCORE_BADGE_BACKGROUND),
            15.0,
            "a player's running score in the Replay Viewer",
        ),
        // ---- Text on controls, in each state a control can be in ----------
        text(
            ink!(TEXT_PRIMARY),
            ink!(BUTTON_BACKGROUND),
            14.0,
            "button labels at rest, down to the Settings volume stops",
        ),
        text(
            ink!(TEXT_PRIMARY),
            ink!(BUTTON_HOVERED),
            14.0,
            "button labels under the pointer",
        ),
        text(
            ink!(TEXT_PRIMARY),
            ink!(BUTTON_PRESSED),
            14.0,
            "button labels while pressed",
        ),
        text(
            ink!(TEXT_PRIMARY),
            ink!(ORANGE_ACTIVE),
            15.0,
            "labels of active controls: an On toggle, a ticked rule, the armed speed, a field being edited",
        ),
        text(
            ink!(TEXT_PRIMARY),
            ink!(BUTTON_DISABLED),
            15.0,
            "labels of the disabled paging buttons in the Library and Help",
        ),
        text(
            ink!(TEXT_MUTED),
            ink!(BUTTON_DISABLED),
            15.0,
            "labels of disabled Viewer controls and Settings fields; the selected Settings tab",
        ),
        text(
            ink!(TEXT_HEADING),
            ink!(BUTTON_BACKGROUND),
            16.0,
            "the bound key or button shown in a Settings binding row",
        ),
        text(
            ink!(TEXT_HEADING),
            ink!(BUTTON_HOVERED),
            16.0,
            "a Settings binding row under the pointer",
        ),
        text(
            ink!(TEXT_HEADING),
            ink!(BUTTON_PRESSED),
            16.0,
            "a Settings binding row while pressed",
        ),
        text(
            ink!(TEXT_HEADING),
            ink!(BUTTON_DISABLED),
            16.0,
            "a controller binding row with no controller assigned",
        ),
        text(
            ink!(TEXT_HEADING),
            ink!(ORANGE_ACTIVE),
            14.0,
            "a binding row waiting for input; the selected volume stop",
        ),
        text(
            ink!(TEXT_PRIMARY),
            ink!(DANGER_BACKGROUND),
            15.0,
            "Delete and Delete Permanently at rest",
        ),
        text(
            ink!(TEXT_PRIMARY),
            ink!(DANGER_HOVERED),
            15.0,
            "Delete and Delete Permanently under the pointer",
        ),
        text(
            ink!(TEXT_PRIMARY),
            ink!(DANGER_PRESSED),
            15.0,
            "Delete and Delete Permanently while pressed",
        ),
        // ---- Text on the countdown cards drawn over a board ---------------
        text(
            ink!(TEXT_HEADING),
            ink!(COUNTDOWN_BACKGROUND).over(ink!(BOARD_WOOD)),
            64.0,
            "the Round 0 countdown over the live board",
        ),
        text(
            ink!(TEXT_HEADING),
            ink!(COUNTDOWN_BACKGROUND).over(ink!(WHITE_STONE)),
            64.0,
            "the same card where a revealed white stone lies beneath Go!, its lightest background",
        ),
        text(
            ink!(TEXT_HEADING),
            ink!(REPLAY_COUNTDOWN_BACKGROUND).over(ink!(BOARD_WOOD)),
            58.0,
            "the Round 0 countdown over the Replay Viewer's board",
        ),
        text(
            ink!(TEXT_HEADING),
            ink!(REPLAY_COUNTDOWN_BACKGROUND).over(ink!(WHITE_STONE)),
            58.0,
            "the same card where a revealed white stone lies beneath it, its lightest background",
        ),
        // ---- The board and its stones -------------------------------------
        graphic(
            ink!(BOARD_WOOD),
            ink!(APP_BACKGROUND),
            "the edge of the live and Replay Viewer boards",
        ),
        graphic(
            ink!(BOARD_WOOD),
            ink!(SURFACE_BACKGROUND),
            "the edge of a thumbnail on its Replay Library card",
        ),
        graphic(
            ink!(BOARD_WOOD),
            ink!(PANEL_BACKGROUND),
            "the wood chip behind each stone, Teleport Marker, hidden-selection mark and the \
             score badges in the Viewer's Key",
        ),
        graphic(
            ink!(BOARD_GRID),
            ink!(BOARD_WOOD),
            "grid lines and star points",
        ),
        graphic(
            ink!(EMPTY_INTERSECTION),
            ink!(BOARD_WOOD),
            "the dot marking an empty intersection",
        ),
        graphic(
            ink!(BLACK_STONE),
            ink!(BOARD_WOOD),
            "a black stone; Player One's cursor border",
        ),
        graphic(
            ink!(STONE_RING),
            ink!(BOARD_WOOD),
            "the ring that draws a white or gray stone's edge",
        ),
        graphic(
            ink!(WHITE_STONE),
            ink!(BOARD_WOOD),
            "a white stone on the board; Player Two's cursor border on an empty point",
        ),
        graphic(
            ink!(WHITE_STONE),
            ink!(STONE_RING),
            "a white stone's fill inside its ring; Player Two's cursor border crossing a ring",
        ),
        graphic(
            ink!(GRAY_STONE),
            ink!(STONE_RING),
            "a gray stone's fill inside its ring",
        ),
        graphic(
            ink!(GRAY_STONE),
            ink!(BLACK_STONE),
            "telling a gray stone from a black one",
        ),
        graphic(
            ink!(GRAY_STONE),
            ink!(WHITE_STONE),
            "telling a gray stone from a white one; Player Two's cursor border over a gray stone",
        ),
        graphic(
            ink!(WHITE_STONE),
            ink!(BLACK_STONE),
            "telling a white stone from a black one; either cursor's border over the other colour's stone",
        ),
        graphic(
            ink!(BLACK_STONE),
            ink!(THUMBNAIL_EMPTY),
            "a black stone in a Replay Library thumbnail",
        ),
        graphic(
            ink!(STONE_RING),
            ink!(THUMBNAIL_EMPTY),
            "a white or gray stone's ring in a thumbnail, against the empty cells around it",
        ),
        // ---- Cursors, badges and the Match screen's own marks -------------
        graphic(
            ink!(BLACK_P1_RED),
            ink!(BLACK_STONE),
            "Player One's cursor accent on its border",
        ),
        graphic(
            ink!(SCORE_BADGE_BACKGROUND),
            ink!(BOARD_WOOD),
            "a Replay Viewer score badge on the board, and the pair of them in the Key",
        ),
        graphic(
            ink!(BLACK_P1_RED),
            ink!(SCORE_BADGE_BACKGROUND),
            "Player One's frame around their score badge, on the board and in the Key",
        ),
        graphic(
            ink!(WHITE_P2_BLUE),
            ink!(SCORE_BADGE_BACKGROUND),
            "Player Two's frame around their score badge, on the board and in the Key",
        ),
        graphic(
            ink!(COUNTDOWN_BACKGROUND).over(ink!(BOARD_WOOD)),
            ink!(BOARD_WOOD),
            "the countdown card on the live board",
        ),
        graphic(
            ink!(REPLAY_COUNTDOWN_BACKGROUND).over(ink!(BOARD_WOOD)),
            ink!(BOARD_WOOD),
            "the countdown card on the Replay Viewer's board",
        ),
        graphic(
            ink!(BLACK_P1_RED),
            ink!(APP_BACKGROUND),
            "the frame of Player One's control hint, and the dividing line and refusal \
             cross of their Round 0 dots",
        ),
        graphic(
            ink!(WHITE_P2_BLUE),
            ink!(APP_BACKGROUND),
            "the frame of Player Two's control hint, and the dividing line and refusal \
             cross of their Round 0 dots",
        ),
        // ---- The Round 0 dots ------------------------------------------------
        // A dot's frame is its player's accent and fades with the dot; its
        // fill runs from the unfilled dot's tone to the accent as it crosses
        // the line. Both ends of both are held here; the tones between are
        // on screen only while a dot moves (see `ui::match_screen::opening_dots`),
        // and the frame, at full strength throughout a crossing, is what
        // carries the dot's edge while they are.
        graphic(
            ink!(BLACK_P1_RED, opening_dot_frame(Player::One, 1.0)),
            ink!(APP_BACKGROUND),
            "the frame of one of Player One's Round 0 dots",
        ),
        graphic(
            ink!(WHITE_P2_BLUE, opening_dot_frame(Player::Two, 1.0)),
            ink!(APP_BACKGROUND),
            "the frame of one of Player Two's Round 0 dots",
        ),
        graphic(
            ink!(BLACK_P1_RED, opening_dot_fill(Player::One, 1.0, 1.0)),
            ink!(
                DEEP_SURFACE_BACKGROUND,
                opening_dot_fill(Player::One, 0.0, 1.0)
            ),
            "a filled Round 0 dot of Player One's against an unfilled one",
        ),
        graphic(
            ink!(WHITE_P2_BLUE, opening_dot_fill(Player::Two, 1.0, 1.0)),
            ink!(
                DEEP_SURFACE_BACKGROUND,
                opening_dot_fill(Player::Two, 0.0, 1.0)
            ),
            "a filled Round 0 dot of Player Two's against an unfilled one",
        ),
        graphic(
            ink!(ORANGE),
            ink!(DEEP_SURFACE_BACKGROUND),
            "the hold-progress fill on its track",
        ),
        graphic(
            ink!(TEXT_HEADING),
            ink!(DEEP_SURFACE_BACKGROUND),
            "the round-time fill on its track",
        ),
        graphic(
            ink!(TEXT_HEADING),
            ink!(APP_BACKGROUND),
            "the outline of the hold-progress track",
        ),
        graphic(
            ink!(TEXT_SUBTLE),
            ink!(APP_BACKGROUND),
            "the outline of the round-time track",
        ),
        // ---- The Viewer's Key ---------------------------------------------
        graphic(
            ink!(BLACK_P1_RED),
            ink!(PANEL_BACKGROUND),
            "Player One's cursor swatch in the Key",
        ),
        graphic(
            ink!(WHITE_P2_BLUE),
            ink!(PANEL_BACKGROUND),
            "Player Two's cursor swatch in the Key",
        ),
        graphic(
            ink!(WHITE_STONE),
            ink!(PANEL_BACKGROUND),
            "the white border of Player Two's cursor swatch in the Key",
        ),
        // ---- The Replay Viewer's marks: Teleport Markers ------------------
        // A diamond of three strokes - `teleport_marker_strokes`, outermost
        // first. It can lie on any point, so on wood and on all three
        // stones; the Key draws the same strokes on a chip of board wood.
        // The outer stroke lies against whatever the marker is on and
        // carries it everywhere but on a stone of its own colour, where it
        // is lost and the innermost stroke, against the same stone seen
        // through the middle, carries it instead: for Player Two their deep
        // blue, for Player One the accent, since their innermost stroke is
        // black again. The accent lies between the other two along every
        // side; only on the faces of the notch at each tip does a pixel and
        // a half of its edge meet the board, which has no row for the same
        // reason the crossings of one mark over another have none.
        graphic(
            ink!(BLACK_STONE),
            ink!(BOARD_WOOD),
            "the outer and innermost strokes of Player One's Teleport Marker on an empty \
             point, and in the Key",
        ),
        graphic(
            ink!(BLACK_STONE),
            ink!(WHITE_STONE),
            "the outer and innermost strokes of Player One's Teleport Marker over a white \
             stone",
        ),
        graphic(
            ink!(BLACK_STONE),
            ink!(GRAY_STONE),
            "the outer and innermost strokes of Player One's Teleport Marker over a gray \
             stone",
        ),
        graphic(
            ink!(BLACK_P1_RED),
            ink!(BLACK_STONE),
            "the accent of Player One's Teleport Marker on the black strokes either side of it, \
             and over a black stone, where both are lost but for the four tips that pass the \
             stone's edge and the accent is all that shows",
        ),
        graphic(
            ink!(WHITE_STONE),
            ink!(BOARD_WOOD),
            "the outer stroke of Player Two's Teleport Marker on an empty point, and in the Key",
        ),
        graphic(
            ink!(WHITE_STONE),
            ink!(BLACK_STONE),
            "the outer stroke of Player Two's Teleport Marker over a black stone",
        ),
        graphic(
            ink!(WHITE_STONE),
            ink!(GRAY_STONE),
            "the outer stroke of Player Two's Teleport Marker over a gray stone",
        ),
        graphic(
            ink!(WHITE_P2_DEEP_BLUE),
            ink!(WHITE_STONE),
            "the innermost stroke of Player Two's Teleport Marker over a white stone, where its \
             white outer stroke is lost: what shows the marker there, which until 2026-10-02 \
             the accent alone did, at 2.75:1",
        ),
        graphic(
            ink!(WHITE_P2_DEEP_BLUE),
            ink!(GRAY_STONE),
            "the innermost stroke of Player Two's Teleport Marker over a gray stone",
        ),
        // ---- The Replay Viewer's marks: hidden selections -----------------
        // Three strokes around a bare middle - `hidden_selection_strokes`,
        // outermost first. The outer stroke lies against whatever the mark
        // is on, the innermost against the same surface seen through the
        // middle, and the accent only ever between those two. A recorded
        // Match puts a selection on an empty point or on the selecting
        // player's own stone; `a_hidden_selection_mark_reads_on_every_surface`
        // holds the mark on the other stones too, where only a hand-edited
        // Replay can put it.
        graphic(
            ink!(BLACK_STONE),
            ink!(BOARD_WOOD),
            "the outer stroke of Player One's hidden-selection mark on an empty point, and in \
             the Key",
        ),
        graphic(
            ink!(BLACK_P1_RED),
            ink!(BLACK_STONE),
            "the accent stroke of Player One's hidden-selection mark against the outer stroke, \
             and against the black stone where a selection of their own stone loses that stroke",
        ),
        graphic(
            ink!(BLACK_P1_PALE_RED),
            ink!(BLACK_P1_RED),
            "the innermost stroke of Player One's hidden-selection mark against the accent \
             stroke around it",
        ),
        graphic(
            ink!(BLACK_P1_PALE_RED),
            ink!(BLACK_STONE),
            "the innermost stroke of Player One's hidden-selection mark against their own black \
             stone, seen through the mark's middle: what shows a selection of that stone",
        ),
        graphic(
            ink!(WHITE_STONE),
            ink!(BOARD_WOOD),
            "the outer stroke of Player Two's hidden-selection mark on an empty point, and in \
             the Key",
        ),
        graphic(
            ink!(WHITE_P2_DEEP_BLUE),
            ink!(WHITE_P2_BLUE),
            "the innermost stroke of Player Two's hidden-selection mark, and of their Teleport \
             Marker, against the accent stroke around it",
        ),
        graphic(
            ink!(WHITE_P2_DEEP_BLUE),
            ink!(WHITE_STONE),
            "the innermost stroke of Player Two's hidden-selection mark against their own white \
             stone, seen through the mark's middle: what shows a selection of that stone",
        ),
        graphic(
            ink!(WHITE_P2_DEEP_BLUE),
            ink!(BOARD_WOOD),
            "the innermost stroke of Player Two's hidden-selection mark, and of their Teleport \
             Marker, against the wood seen through its middle, on an empty point and in the Key",
        ),
        graphic(
            ink!(WHITE_STONE),
            ink!(BLACK_STONE),
            "telling the two players' Replay Viewer marks apart without hue: the outer stroke \
             of a Teleport Marker and of a hidden-selection mark is black for Player One and \
             white for Player Two",
        ),
        // ---- Control borders, frames and the focus ring -------------------
        graphic(
            ink!(BLUE_BORDER),
            ink!(APP_BACKGROUND),
            "button borders on the Replay Viewer and Replay Library screens",
        ),
        graphic(
            ink!(BLUE_BORDER),
            ink!(PANEL_BACKGROUND),
            "button and field borders inside panels and dialogs",
        ),
        graphic(
            ink!(BLUE_BORDER),
            ink!(SURFACE_BACKGROUND),
            "control borders in Settings and on Replay Library cards",
        ),
        graphic(
            ink!(BLUE_BORDER),
            home_over_panel(),
            "the Home menu buttons' borders",
        ),
        graphic(
            ink!(BLUE_BORDER),
            home_over_orange(),
            "a Home menu button's border where the orange band passes beneath the panel",
        ),
        graphic(
            ink!(BLUE_BORDER),
            home_over_app(),
            "the ends of the last Home menu button's border, beyond both diamonds",
        ),
        graphic(
            ink!(DANGER),
            ink!(SURFACE_BACKGROUND),
            "the Delete button's border on a Replay Library card",
        ),
        graphic(
            ink!(DANGER),
            ink!(PANEL_BACKGROUND),
            "the Delete Permanently button's border in the delete dialog",
        ),
        graphic(
            ink!(ORANGE),
            ink!(APP_BACKGROUND),
            "the focus ring, and the armed-speed border, on the Viewer and Library screens; the Home diamond",
        ),
        graphic(
            ink!(ORANGE),
            ink!(PANEL_BACKGROUND),
            "the focus ring inside panels and dialogs; modal and panel frames",
        ),
        graphic(
            ink!(ORANGE),
            ink!(SURFACE_BACKGROUND),
            "the focus ring, and an active control's border, in Settings and on Library cards",
        ),
        graphic(
            ink!(ORANGE),
            home_over_panel(),
            "the focus ring around a Home menu button",
        ),
        graphic(
            ink!(ORANGE),
            home_over_orange(),
            "the same ring where the orange band passes beneath the panel",
        ),
        graphic(
            ink!(ORANGE),
            home_over_app(),
            "the same ring around the last Home menu button, beyond both diamonds",
        ),
        graphic(
            ink!(ORANGE),
            ink!(ORANGE_ACTIVE),
            "an active control's border against its own fill",
        ),
        graphic(
            ink!(ORANGE),
            scrim(),
            "a modal's frame against the scrim around it",
        ),
        graphic(
            ink!(ORANGE),
            scrim_over_white(),
            "the same frame where the scrim covers a white stone, its lightest background",
        ),
        graphic(
            ink!(MARIMBA_BAR_STRUCK, marimba_bar(1.0, 1.0)),
            ink!(MARIMBA_BAR, marimba_bar(1.0, 0.0)),
            "a struck bar of the Home marimba against the bars at rest",
        ),
        // ---- The capture animation's fading disc --------------------------
        // The disc fades from the captured stone's colour to its captor's,
        // so its two ends are the two stone colours, both already held to
        // the wood above; in between it carries the grid's ink ring, which
        // is what keeps its edge on the wood while the fill crosses the
        // wood's own tone (see `ui::match_screen::capture`).
        graphic(
            ink!(BLACK_STONE, captured_stone_fill(Player::One, 0.0, 1.0)),
            ink!(BOARD_WOOD),
            "a captured black stone as its fade begins, on the board",
        ),
        graphic(
            ink!(WHITE_STONE, captured_stone_fill(Player::One, 1.0, 1.0)),
            ink!(BOARD_WOOD),
            "the same stone at the end of its fade, in its captor's white",
        ),
    ]
}

/// Every pairing that occurs on screen and does not meet the floor.
fn exemptions() -> Vec<Exemption> {
    const HUE_ONLY: &str = "the active fill and border differ from the resting ones in hue, \
        not lightness";
    const ACTIVE_STATE_IN_WORDS: &str = "a word or a glyph changes with the state: On or Off, \
        an X in the box, Press a key..., a caret in the field, Pause in place of Play, and \
        brackets around the armed speed and the selected volume stop, as [1x] and [Medium]";
    const OWN_STONE: &str = "the stroke is the stone's own colour, which is what makes it the \
        player's: on that stone it cannot be seen at all";

    vec![
        // ---- Stones against the board: carried by the ring ----------------
        exempt(
            graphic(
                ink!(GRAY_STONE),
                ink!(BOARD_WOOD),
                "a gray stone on the board",
            ),
            "no gray can stand 3:1 clear of black, white and the board at once: the three \
             stones confine it to a band whose widest ratio against any board is 1.98:1",
            Excuse::SecondChannel(
                "the stone's ring - STONE_RING on BOARD_WOOD, and GRAY_STONE on STONE_RING, \
                 both declared above - and its smaller footprint",
            ),
        ),
        exempt(
            graphic(
                ink!(GRAY_STONE),
                ink!(THUMBNAIL_EMPTY),
                "a gray stone in a Replay Library thumbnail",
            ),
            "the thumbnail's empty cells are darker wood than the board, and closer to gray still",
            Excuse::SecondChannel(
                "the cell's ring - STONE_RING on THUMBNAIL_EMPTY, and GRAY_STONE on \
                 STONE_RING, both declared above",
            ),
        ),
        exempt(
            graphic(
                ink!(EMPTY_INTERSECTION),
                ink!(BOARD_GRID),
                "the empty-intersection dot where it lies on a star point",
            ),
            "the dot and the star point are nearly the same ink",
            Excuse::SecondChannel(
                "an empty point is known by carrying no stone, and the star point around \
                 the dot is itself BOARD_GRID on BOARD_WOOD",
            ),
        ),
        // ---- The two players' accents -------------------------------------
        exempt(
            graphic(
                ink!(WHITE_P2_BLUE),
                ink!(WHITE_STONE),
                "Player Two's accent on the white border of their cursor, and beside the white \
                 outer stroke of their Teleport Marker and of their hidden-selection mark",
            ),
            "the accent names the player, and blue is not 3:1 from the border it sits on",
            Excuse::SecondChannel(
                "the two cursors differ in size (68 against 52), the two markers likewise (40 \
                 against 20), and all three marks in their outside stroke, black against white, \
                 which WHITE_STONE on BLACK_STONE carries",
            ),
        ),
        exempt(
            graphic(
                ink!(BLACK_P1_RED),
                ink!(WHITE_P2_BLUE),
                "telling the two players' accents apart: cursors, Teleport Markers, \
                 hidden-selection marks, sidebars, hint cards, Round 0 dots, binding columns",
            ),
            "the two identity colours are close in lightness",
            Excuse::SecondChannel(
                "in each of these places the players also differ by something else: a black or \
                 white outside stroke on the board, and for cursors and markers size as well; \
                 side of the screen and a written name elsewhere",
            ),
        ),
        exempt(
            graphic(
                ink!(BLACK_P1_RED),
                ink!(WHITE_P2_BLUE),
                "the Replay Viewer's two score badges when both cursors share a point, and \
                 the Key's swatch of the pair",
            ),
            "the badges are identical but for a frame in the player's accent, and with the \
             cursors together they sit side by side",
            Excuse::SecondChannel(
                "position: side by side, Player One's badge is always the left one, the order \
                 the Viewer's result line names the players in, Black then White, and \
                 `the_badges_keep_their_gaps_and_order_in_window_pixels_at_every_size` holds it \
                 on every point of the board; the Key's row for the badges says so in words, and \
                 `the_keys_badge_row_names_the_order_the_board_draws` holds that row to the board",
            ),
        ),
        // ---- Which control is active --------------------------------------
        exempt(
            graphic(
                ink!(ORANGE_ACTIVE),
                ink!(BUTTON_BACKGROUND),
                "an active control's fill against a resting one's: Settings toggles, ticked \
                 rules, binding rows and fields; a running Play; the armed speed in the Replay \
                 Viewer and the selected volume stop in Settings, against the rest of their group",
            ),
            HUE_ONLY,
            Excuse::SecondChannel(ACTIVE_STATE_IN_WORDS),
        ),
        exempt(
            graphic(
                ink!(ORANGE),
                ink!(BLUE_BORDER),
                "the same controls' active border against a resting one's",
            ),
            HUE_ONLY,
            Excuse::SecondChannel(ACTIVE_STATE_IN_WORDS),
        ),
        exempt(
            graphic(
                ink!(BUTTON_HOVERED),
                ink!(BUTTON_BACKGROUND),
                "a button under the pointer against one at rest",
            ),
            "the hover fill is a tint of the resting one",
            Excuse::SecondChannel(
                "the pointer itself is on the control; keyboard and controller focus is the \
                 ORANGE ring, declared above",
            ),
        ),
        exempt(
            graphic(
                ink!(BUTTON_PRESSED),
                ink!(BUTTON_BACKGROUND),
                "a button being pressed against one at rest",
            ),
            "the press fill is a tint of the resting one",
            Excuse::SecondChannel("the press is the player's own, and the control acts on it"),
        ),
        exempt(
            graphic(
                ink!(DANGER_HOVERED),
                ink!(DANGER_BACKGROUND),
                "a Delete button under the pointer against one at rest",
            ),
            "the hover fill is a tint of the resting one",
            Excuse::SecondChannel(
                "the pointer itself is on the control; keyboard and controller focus is the \
                 ORANGE ring, declared above",
            ),
        ),
        exempt(
            graphic(
                ink!(DANGER_PRESSED),
                ink!(DANGER_BACKGROUND),
                "a Delete button being pressed against one at rest",
            ),
            "the press fill is a tint of the resting one",
            Excuse::SecondChannel("the press is the player's own, and the control acts on it"),
        ),
        // ---- Two bars that share a slot -----------------------------------
        exempt(
            graphic(
                ink!(ORANGE),
                ink!(TEXT_HEADING),
                "the hold-progress fill against the round-time fill, which take turns in the \
                 slot below the board",
            ),
            "one bar is the accent orange and the other the clock's cream, chosen to differ \
             in hue; they are never on screen together",
            Excuse::SecondChannel(
                "the hold bar never appears without its label above it, Ending Match or \
                 Restarting Match, and it grows while the round-time bar shrinks",
            ),
        ),
        // ---- A control with no border -------------------------------------
        exempt(
            graphic(
                ink!(BUTTON_BACKGROUND),
                ink!(APP_BACKGROUND),
                "the Match screen's Home and Help buttons, which are drawn without a border",
            ),
            "the two buttons set a border colour but no border width, so their fill is their \
             only edge",
            Excuse::SecondChannel(
                "the label identifies the control - TEXT_PRIMARY on BUTTON_BACKGROUND, declared \
                 above - and WCAG asks no visible boundary of a labelled button",
            ),
        ),
        // ---- Controls that are switched off -------------------------------
        exempt(
            graphic(
                ink!(BUTTON_DISABLED),
                ink!(BUTTON_BACKGROUND),
                "a disabled control's fill against an enabled one's",
            ),
            "a disabled control is drawn receding into the surface on purpose",
            Excuse::SecondChannel(
                "what disables it is on screen in words or in place: the Viewer's timer at \
                 the start or the end, the Library's page line, the Help page number, an \
                 unticked rule, No controller assigned, the selected tab's own panel on show",
            ),
        ),
        exempt(
            graphic(
                ink!(BUTTON_DISABLED_BORDER),
                ink!(APP_BACKGROUND),
                "the border of a disabled Replay Viewer control",
            ),
            "a disabled control is drawn receding into the surface on purpose",
            Excuse::NotRequired(
                "an inactive control; its label stays readable, as TEXT_MUTED on \
                 BUTTON_DISABLED above",
            ),
        ),
        exempt(
            graphic(
                ink!(BUTTON_DISABLED_BORDER),
                ink!(PANEL_BACKGROUND),
                "the border of the selected Settings tab, which cannot be chosen again",
            ),
            "a disabled control is drawn receding into the surface on purpose",
            Excuse::NotRequired(
                "an inactive control; its label stays readable, and the tab's own panel is \
                 the one on show beneath it",
            ),
        ),
        exempt(
            graphic(
                ink!(BUTTON_DISABLED_BORDER),
                ink!(SURFACE_BACKGROUND),
                "the border of a disabled Settings field or binding row",
            ),
            "a disabled control is drawn receding into the surface on purpose",
            Excuse::NotRequired(
                "an inactive control; its label stays readable, as TEXT_MUTED and \
                 TEXT_HEADING on BUTTON_DISABLED above",
            ),
        ),
        // ---- The Replay Viewer's marks ------------------------------------
        // Each mark is three strokes so that a stroke which fails on one
        // surface has another beside it that does not. These are the
        // strokes that fail, each with the one that carries it.
        //
        // Where two marks share a point one is drawn over the other - a
        // selection over a marker, a marker over a cursor - and a stroke of
        // one then lies against a stroke of the other for a few pixels.
        // Those crossings have no rows, here or in `pairs`. Each mark is
        // carried on a shared point as it is alone, by the strokes of it
        // that lie against the board: no cursor or marker covers any of a
        // selection's mark, and what is covered of a marker or a cursor is
        // the lesser part of its outline. `docs/ARCHITECTURE.md` gives the
        // measured shares, and where the cover is heaviest.
        exempt(
            graphic(
                ink!(BLACK_STONE),
                ink!(BLACK_STONE),
                "the outer and innermost strokes of Player One's Teleport Marker, and the outer \
                 stroke of their hidden-selection mark, over a black stone",
            ),
            OWN_STONE,
            Excuse::SecondChannel(
                "the stroke inside it: BLACK_P1_RED on BLACK_STONE, declared above for both \
                 marks, and for a hidden selection BLACK_P1_PALE_RED on BLACK_STONE as well",
            ),
        ),
        exempt(
            graphic(
                ink!(WHITE_STONE),
                ink!(WHITE_STONE),
                "the outer stroke of Player Two's Teleport Marker, and of their hidden-selection \
                 mark, over a white stone, which leaves the accent stroke against that stone at \
                 2.75:1",
            ),
            OWN_STONE,
            Excuse::SecondChannel(
                "the mark's innermost stroke: WHITE_P2_DEEP_BLUE on WHITE_STONE, and on the \
                 accent stroke beside it, both declared above for both marks",
            ),
        ),
        exempt(
            graphic(
                ink!(BLACK_P1_PALE_RED),
                ink!(BOARD_WOOD),
                "the innermost stroke of Player One's hidden-selection mark against the wood \
                 seen through its middle, on an empty point and in the Key",
            ),
            "3:1 above this wood needs a luminance of 0.85, all but a white stone's 0.88. \
             The stroke was kept visibly a red, at 0.77, as the owner asked of every stroke \
             of a player's mark",
            Excuse::SecondChannel(
                "the mark's outer stroke, which is what the inner one is there to stand in \
                 for on a black stone: BLACK_STONE on BOARD_WOOD, declared above",
            ),
        ),
        exempt(
            graphic(
                ink!(WHITE_P2_DEEP_BLUE),
                ink!(BLACK_STONE),
                "the innermost stroke of Player Two's Teleport Marker over a black stone, seen \
                 through its middle",
            ),
            "the stroke is there for a white stone, and is as dark as a black one",
            Excuse::SecondChannel(
                "the marker's outer stroke, which the inner one stands in for only on a white \
                 stone: WHITE_STONE on BLACK_STONE, declared above",
            ),
        ),
        // ---- Key swatches a sentence names --------------------------------
        exempt(
            graphic(
                ink!(BLACK_STONE),
                ink!(PANEL_BACKGROUND),
                "the black border of Player One's cursor swatch in the Key",
            ),
            "the swatch shows the cursor's own colours, and its border is the panel's lightness",
            Excuse::SecondChannel(
                "the sentence beside the swatch names it, and its red fill is BLACK_P1_RED on \
                 PANEL_BACKGROUND, declared above",
            ),
        ),
        // ---- Decoration ---------------------------------------------------
        exempt(
            graphic(
                ink!(CARD_BORDER),
                ink!(DEEP_SURFACE_BACKGROUND),
                "the frame of a Replay Library card against the list behind it",
            ),
            "a quiet frame, so that a page of cards does not read as a page of boxes",
            Excuse::SecondChannel(
                "each card is a row of its own - a thumbnail, its lines of text and its three \
                 buttons - with a gap before the next",
            ),
        ),
        exempt(
            graphic(
                ink!(THUMBNAIL_GRID).over(ink!(BOARD_WOOD)),
                ink!(THUMBNAIL_EMPTY),
                "the faint line between two cells of a thumbnail, against the cells it divides",
            ),
            "a translucent line over the thumbnail's wood, which comes out close to the empty \
             cells' own colour",
            Excuse::NotRequired("decoration: the cells are told apart by their fills and rings"),
        ),
        exempt(
            graphic(
                ink!(THUMBNAIL_EMPTY),
                ink!(BOARD_WOOD),
                "a thumbnail's grid against the wood frame around it",
            ),
            "two woods, so the frame reads as the board's own edge",
            Excuse::NotRequired("decoration: the frame marks nothing a player needs"),
        ),
        exempt(
            graphic(
                ink!(MARIMBA_BAR),
                ink!(APP_BACKGROUND),
                "a bar of the Home marimba at rest",
            ),
            "dark wood on a dark background, so the Easter egg stays behind the menu",
            Excuse::NotRequired(
                "decoration: the marimba appears only once twenty notes are played and carries \
                 no game information; a struck bar brightens to MARIMBA_BAR_STRUCK, declared above",
            ),
        ),
        exempt(
            graphic(
                ink!(MARIMBA_BORDER, marimba_border(1.0)).over(ink!(APP_BACKGROUND)),
                ink!(MARIMBA_BAR),
                "the frame of a marimba bar against the bar inside it",
            ),
            "a darker edge of the same wood",
            Excuse::NotRequired("decoration: see MARIMBA_BAR on APP_BACKGROUND"),
        ),
    ]
}

/// The names of the `Color` constants `presentation.rs` declares, read from
/// its source.
fn declared_colour_constants() -> Vec<String> {
    source_file("presentation.rs")
        .lines()
        .filter_map(|line| {
            let declaration = line
                .strip_prefix("pub(crate) const ")
                .or_else(|| line.strip_prefix("const "))?;
            let (name, rest) = declaration.split_once(':')?;
            rest.trim_start()
                .starts_with("Color =")
                .then(|| name.trim().to_string())
        })
        .collect()
}

/// The names of the functions in `presentation.rs` that make a colour of
/// their own, rather than choosing among the constants: those whose return
/// type holds a `Color` and whose body constructs or derives one, found with
/// the same scan
/// [`crate::architecture`] runs over the rest of the crate.
///
/// A top-level function is recognised by its signature starting a line and
/// its closing brace standing alone on one, which is how rustfmt writes
/// every function in that file.
fn colour_making_functions() -> Vec<String> {
    let source = source_file("presentation.rs");
    let made_on: Vec<usize> = colour_constructions(&source)
        .into_iter()
        .map(|(line, _)| line)
        .collect();
    let lines: Vec<&str> = source.lines().collect();
    let mut found = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let signature = ["pub(crate) const fn ", "pub(crate) fn ", "const fn ", "fn "]
            .iter()
            .find_map(|prefix| lines[index].strip_prefix(prefix));
        let Some(signature) = signature else {
            index += 1;
            continue;
        };
        let end = (index..lines.len())
            .find(|line| lines[*line] == "}")
            .expect("a top-level function ends with a brace on its own line");
        let header_end = (index..=end)
            .find(|line| lines[*line].ends_with('{'))
            .unwrap_or(end);
        let returns_colour = lines[index..=header_end]
            .iter()
            .any(|line| returns_a_colour(line));
        // `made_on` is one-based, so line `index` of the file is `index + 1`.
        let makes_one = made_on
            .iter()
            .any(|line| (index + 1..=end + 1).contains(line));
        if returns_colour && makes_one {
            let name = signature.split('(').next().unwrap_or_default().trim();
            found.push(name.to_string());
        }
        index = end + 1;
    }
    found
}

/// Whether a line of a function's header names `Color` in its return type:
/// a bare `-> Color`, or a type that holds one, as `-> [Color; 3]` does.
fn returns_a_colour(header_line: &str) -> bool {
    header_line
        .split_once("->")
        .is_some_and(|(_, returned)| names(returned, "Color"))
}

/// Whether `name` occurs in `text` as a whole identifier, so that
/// `BUTTON_DISABLED` is not found inside `BUTTON_DISABLED_BORDER`.
fn names(text: &str, name: &str) -> bool {
    let is_identifier = |character: char| character.is_ascii_alphanumeric() || character == '_';
    text.match_indices(name).any(|(start, found)| {
        let before = text[..start].chars().next_back();
        let after = text[start + found.len()..].chars().next();
        !before.is_some_and(is_identifier) && !after.is_some_and(is_identifier)
    })
}

#[test]
fn every_declared_pairing_meets_the_floor() {
    let failing: Vec<String> = pairs()
        .iter()
        .filter(|pair| !pair.meets_the_floor())
        .map(Pair::describe)
        .collect();
    assert!(
        failing.is_empty(),
        "these pairings are below the contrast floor (CONTRIBUTING.md, \"UI and \
         accessibility\"). Change the colour; if it cannot change, move the row to \
         `exemptions` with a reason and say what else carries the meaning:\n  {}",
        failing.join("\n  ")
    );
}

#[test]
fn every_exemption_is_still_needed() {
    let stale: Vec<String> = exemptions()
        .iter()
        .filter(|exemption| exemption.pair.meets_the_floor())
        .map(|exemption| exemption.pair.describe())
        .collect();
    assert!(
        stale.is_empty(),
        "these exempt pairings now meet the floor. An exemption that is no longer needed \
         is a claim that something is still wrong, so move each row to `pairs`:\n  {}",
        stale.join("\n  ")
    );
}

#[test]
fn every_exemption_says_why_and_what_else_carries_it() {
    for exemption in exemptions() {
        let pair = exemption.pair.describe();
        assert!(exemption.reason.len() >= 20, "no real reason given: {pair}");
        match exemption.excuse {
            Excuse::SecondChannel(channel) | Excuse::NotRequired(channel) => {
                assert!(channel.len() >= 20, "no real channel named: {pair}");
            }
            Excuse::AwaitingOwnerDecision => {}
        }
    }
}

/// The exemptions in `rows` that nothing carries yet, described.
fn awaiting_owner_decision(rows: &[Exemption]) -> Vec<String> {
    rows.iter()
        .filter(|exemption| matches!(exemption.excuse, Excuse::AwaitingOwnerDecision))
        .map(|exemption| exemption.pair.describe())
        .collect()
}

#[test]
fn undecided_exemptions_are_counted() {
    let awaiting = awaiting_owner_decision(&exemptions());
    assert_eq!(
        awaiting.len(),
        AWAITING_OWNER_DECISION,
        "the number of pairings below the floor with nothing else carrying their meaning \
         has changed. If one was resolved, lower AWAITING_OWNER_DECISION and update \
         docs/ideas/accessibility-findings.md; adding one is a decision for the owner, not \
         a count to bump. They are:\n  {}",
        awaiting.join("\n  ")
    );
}

/// A count pinned at zero is only an assertion if the count would see a
/// row. This one is not in the table - it is the kind of row the table
/// may need next - and it is the only place [`Excuse::AwaitingOwnerDecision`]
/// is constructed while the table has none.
#[test]
fn a_pairing_awaiting_a_decision_would_be_counted() {
    let pending = exempt(
        graphic(
            ink!(BLACK_P1_RED),
            ink!(WHITE_P2_BLUE),
            "a mark nothing yet tells apart from its neighbour but hue",
        ),
        "the two identity colours are close in lightness",
        Excuse::AwaitingOwnerDecision,
    );
    assert_eq!(awaiting_owner_decision(&[pending]).len(), 1);
}

#[test]
fn every_colour_constant_is_in_the_table() {
    let constants = declared_colour_constants();
    assert!(
        constants.len() >= 40,
        "found only {} colour constants in presentation.rs: suspect the scan before the \
         table. Found: {constants:?}",
        constants.len()
    );
    let rows: Vec<String> = pairs()
        .into_iter()
        .chain(exemptions().into_iter().map(|exemption| exemption.pair))
        .map(|pair| format!("{} {}", pair.foreground.name, pair.background.name))
        .collect();
    let unlisted: Vec<&String> = constants
        .iter()
        .filter(|constant| !rows.iter().any(|row| names(row, constant)))
        .collect();
    assert!(
        unlisted.is_empty(),
        "these colours in presentation.rs appear in no pairing. Say where each is drawn by \
         adding a row to `pairs` in src/contrast.rs - or to `exemptions`, with a reason, if \
         it cannot meet the floor: {unlisted:?}"
    );
}

/// A constant is not the only way `presentation.rs` can make a colour: a
/// function there can build one, as the marimba's two do, and the
/// architecture guard does not look inside that file. So every such
/// function has to be named in a row too.
#[test]
fn every_colour_made_by_a_function_is_in_the_table() {
    let functions = colour_making_functions();
    for known in [
        "marimba_bar",
        "marimba_border",
        "captured_stone_fill",
        "opening_dot_fill",
        "opening_dot_frame",
    ] {
        assert!(
            functions.iter().any(|function| function == known),
            "the scan no longer finds {known}, which makes a colour: suspect the scan \
             before the table. Found: {functions:?}"
        );
    }
    for selector in [
        "player_accent",
        "hidden_selection_strokes",
        "teleport_marker_strokes",
        "button_background",
        "danger_button_background",
    ] {
        assert!(
            !functions.iter().any(|function| function == selector),
            "{selector} only chooses among constants and should not be found: {functions:?}"
        );
    }
    let rows: Vec<String> = pairs()
        .into_iter()
        .chain(exemptions().into_iter().map(|exemption| exemption.pair))
        .map(|pair| format!("{} {}", pair.foreground.name, pair.background.name))
        .collect();
    let unlisted: Vec<&String> = functions
        .iter()
        .filter(|function| !rows.iter().any(|row| names(row, function)))
        .collect();
    assert!(
        unlisted.is_empty(),
        "these functions in presentation.rs make a colour that appears in no pairing. Add a \
         row to `pairs` or `exemptions` in src/contrast.rs whose colour is the function's \
         call: {unlisted:?}"
    );
}

#[test]
fn no_pairing_is_listed_twice() {
    let mut seen = std::collections::BTreeSet::new();
    for pair in pairs()
        .into_iter()
        .chain(exemptions().into_iter().map(|exemption| exemption.pair))
    {
        let key = (
            pair.foreground.name.clone(),
            pair.background.name.clone(),
            pair.place,
        );
        assert!(seen.insert(key), "listed twice: {}", pair.describe());
    }
}

/// The ring is the answer the stone exemptions give, so it is held to the
/// floor by name rather than only as three rows among a hundred: the rim
/// against the board, and each ringed fill against the rim.
#[test]
fn the_ring_carries_what_the_stone_fills_cannot() {
    assert!(contrast(STONE_RING, BOARD_WOOD) >= 3.0);
    assert!(contrast(WHITE_STONE, STONE_RING) >= 3.0);
    assert!(contrast(GRAY_STONE, STONE_RING) >= 3.0);
    assert!(contrast(STONE_RING, THUMBNAIL_EMPTY) >= 3.0);
    assert!(
        contrast(GRAY_STONE, BOARD_WOOD) < 3.0,
        "gray now clears the board on its own; the ring's reason needs restating"
    );
}

/// What a hidden-selection mark promises, held as a property of
/// `hidden_selection_strokes` itself rather than only as rows that name
/// the same constants: whatever the mark lies on, one of the two strokes
/// that touch that surface - the outer one, or the innermost against what
/// shows through the middle - stands 3:1 clear of it.
///
/// A recorded Match only ever puts a selection on wood or on the selecting
/// player's own stone, and the rows cover those. The other stones are held
/// here because `Replay::validate` does not rule them out of every frame
/// of a hand-edited file, and a mark drawn there should still be seen. As
/// of 2026-10-01 the worst of the eight is 4.23:1, Player One's on gray.
///
/// The rest is what makes the mark say whose it is without hue: the two
/// outer strokes are 3:1 apart, so are the two inner ones, and each mark
/// runs the opposite way from the other - dark outside and light inside
/// for Player One, the reverse for Player Two.
#[test]
fn a_hidden_selection_mark_reads_on_every_surface() {
    let surfaces = [
        ("BOARD_WOOD", BOARD_WOOD),
        ("BLACK_STONE", BLACK_STONE),
        ("WHITE_STONE", WHITE_STONE),
        ("GRAY_STONE", GRAY_STONE),
    ];
    for player in Player::ALL {
        let [outer, accent, inner] = hidden_selection_strokes(player);
        for (name, surface) in surfaces {
            let best = contrast(outer, surface).max(contrast(inner, surface));
            assert!(
                best >= 3.0,
                "{player:?}'s hidden-selection mark on {name}: neither its outer nor its \
                 innermost stroke is 3:1 clear ({best:.2}:1)"
            );
        }
        assert!(
            contrast(inner, accent) >= 3.0,
            "{player:?}'s innermost stroke cannot be told from the accent stroke around it"
        );
        assert!(contrast(outer, inner) >= 3.0);
    }
    let [one_outer, _, one_inner] = hidden_selection_strokes(Player::One);
    let [two_outer, _, two_inner] = hidden_selection_strokes(Player::Two);
    assert!(contrast(one_outer, two_outer) >= 3.0);
    assert!(contrast(one_inner, two_inner) >= 3.0);
    assert!(luminance(one_outer) < luminance(one_inner));
    assert!(luminance(two_outer) > luminance(two_inner));
}

/// The same question of a Teleport Marker, which can lie on any point, so
/// on wood and on all three stones. Until 2026-10-02 it was two strokes, a
/// border with the accent along it, and on a white stone Player Two's
/// white border was lost and their accent alone was 2.75:1 - the one case
/// in which no stroke of either mark cleared the floor, held here as
/// failing while it awaited the owner. The third, innermost stroke is what
/// answered it. A marker's property is a little weaker than the ring's,
/// because Player One's innermost stroke is black again: on every surface
/// one of its three strokes is 3:1 clear, and on a stone of the outer
/// stroke's colour that is a stroke which then lies against the stone -
/// Player Two's innermost, seen through the middle, or Player One's accent,
/// between two strokes the stone has swallowed.
#[test]
fn a_teleport_marker_reads_on_every_surface() {
    let surfaces = [
        ("BOARD_WOOD", BOARD_WOOD),
        ("BLACK_STONE", BLACK_STONE),
        ("WHITE_STONE", WHITE_STONE),
        ("GRAY_STONE", GRAY_STONE),
    ];
    for player in Player::ALL {
        let [outer, accent, inner] = teleport_marker_strokes(player);
        assert_eq!(accent, player_accent(player));
        for (name, surface) in surfaces {
            let best = contrast(outer, surface)
                .max(contrast(accent, surface))
                .max(contrast(inner, surface));
            assert!(
                best >= 3.0,
                "{player:?}'s Teleport Marker on {name}: no stroke of it is 3:1 clear \
                 ({best:.2}:1)"
            );
        }
        assert!(contrast(inner, accent) >= 3.0);
    }
    // The case that was open, by name: a palette change that loses it
    // should say so here, beside the reason.
    let [_, accent, inner] = teleport_marker_strokes(Player::Two);
    assert!(
        contrast(accent, WHITE_STONE) < 3.0,
        "the accent carries it now; restate why"
    );
    assert!(contrast(inner, WHITE_STONE) >= 3.0);
}

#[test]
fn the_arithmetic_matches_known_values() {
    let close = |left: f32, right: f32| (left - right).abs() < 0.005;
    assert!(close(contrast(Color::WHITE, Color::BLACK), 21.0));
    assert!(close(contrast(Color::WHITE, Color::WHITE), 1.0));
    // The published boundary case: #767676 is the lightest gray that is
    // 4.5:1 on white.
    let gray = Color::srgb_u8(0x76, 0x76, 0x76);
    assert!(close(contrast(gray, Color::WHITE), 4.54));
    assert!(close(luminance(Color::srgb(1.0, 0.0, 0.0)), 0.2126));

    assert_eq!(Need::Text(23.9).floor(), 4.5);
    assert_eq!(Need::Text(24.0).floor(), 3.0);
    assert_eq!(Need::Graphic.floor(), 3.0);

    // Measured, not derived: the Home panel over the orange diamond was
    // read from a capture of the running game on 2026-09-30 as sRGB
    // (56, 34, 23), and read the same on 2026-10-27 after the upgrade to
    // Bevy 0.20, whose 2D camera defaults to `Tonemapping::Linear` rather
    // than `None` and whose sprites - the diamond under the panel among
    // them - now draw as meshes. Blending the encoded values instead of
    // linear light would give (23, 22, 22), and every contrast against it
    // about a fifth higher than the player actually gets - enough to pass
    // text that fails.
    let composite = ink!(HOME_PANEL_BACKGROUND)
        .over(ink!(ORANGE))
        .color
        .to_srgba();
    let eight_bit = |channel: f32| (channel * 255.0).round() as u8;
    assert_eq!(
        (
            eight_bit(composite.red),
            eight_bit(composite.green),
            eight_bit(composite.blue)
        ),
        (56, 34, 23)
    );
}

/// `hidden_selection_strokes` returns three colours, not one. The scan has
/// to see such a function, or it could build a colour inline and owe the
/// table nothing.
#[test]
fn a_colour_anywhere_in_a_return_type_is_seen() {
    assert!(returns_a_colour(
        "pub(crate) fn marimba_border(alpha: f32) -> Color {"
    ));
    assert!(returns_a_colour(
        "pub(crate) const fn hidden_selection_strokes(player: Player) -> [Color; 3] {"
    ));
    assert!(returns_a_colour(") -> Option<Color> {"));
    assert!(!returns_a_colour(
        "pub(crate) fn darkness(color: Color) -> f32 {"
    ));
    assert!(!returns_a_colour("fn label_colour() -> TextColor {"));
    assert!(!returns_a_colour(
        "pub(crate) fn button_background(highlight: Highlight,"
    ));
}

#[test]
fn a_name_is_matched_as_a_whole_identifier() {
    assert!(names("BUTTON_DISABLED TEXT_MUTED", "BUTTON_DISABLED"));
    assert!(!names("BUTTON_DISABLED_BORDER PANEL", "BUTTON_DISABLED"));
    assert!(!names(
        "HOME_PANEL_BACKGROUND over ORANGE",
        "PANEL_BACKGROUND"
    ));
    assert!(names("HOME_PANEL_BACKGROUND over ORANGE", "ORANGE"));
    assert!(!names("ORANGE_ACTIVE", "ORANGE"));
    assert!(names(
        "MARIMBA_BORDER=marimba_border(1.0) over APP_BACKGROUND",
        "APP_BACKGROUND"
    ));
    assert!(names(
        "MARIMBA_BORDER=marimba_border(1.0) over APP_BACKGROUND",
        "marimba_border"
    ));
    assert!(names("MARIMBA_BAR=marimba_bar(1.0,0.0)", "MARIMBA_BAR"));
    assert!(names("MARIMBA_BAR=marimba_bar(1.0,0.0)", "marimba_bar"));
    assert!(!names(
        "MARIMBA_BAR_STRUCK=marimba_bar(1.0,1.0)",
        "MARIMBA_BAR"
    ));
}
