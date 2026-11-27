//! How a button is pressed: the pointer half of every button in the crate,
//! and the one activation that the pointer, the keyboard and the controller
//! all produce.
//!
//! # One activation, many readers
//!
//! A button is an entity carrying `bevy_ui_widgets`' [`Button`] beside the
//! intent component its screen reads (`HudAction`, `LibraryAction`,
//! `ViewerAction` and the Settings controls). Pressing one, by any input,
//! triggers Bevy's [`Activate`] on it, and nothing else: the pointer here, on
//! release, and the focus ring in `ui::focus`, on the press of Enter, Space
//! or a pad's South. [`forward_activation`] turns each `Activate` into one
//! [`ButtonActivation`] message, and that message is what the four button
//! handlers (`handle_ui_buttons`, `handle_mapping_buttons`,
//! `handle_library_buttons`, `handle_viewer_buttons`) and the Help modal's
//! keys read, so none of them knows or cares which input it was. The
//! activation cue is the one reader that is not a message reader:
//! `crate::audio` observes `Activate` directly, because `audio` may not name
//! `ui`.
//!
//! Both producers run in `PreUpdate` - the pointer's in [`PointerActivation`],
//! just after picking's systems, and the ring after that - and the handlers
//! read in `Update`, so
//! an activation is applied in the frame it happened, after every producer
//! has spoken. That ordering is what the same-frame guarantees in
//! `ui::focus` rest on: a same-frame Escape and Enter close a modal once,
//! and the frame that closes a modal cannot press the button underneath. A
//! handler that is gated to its screen can see an activation from the frame
//! before it started running - a message lives for two frames - but only
//! for a button of the screen it left, which has despawned by then, so the
//! lookup of its intent finds nothing.
//!
//! [`Activated`] is how a handler reads the activations of one kind of
//! button; the Settings router, which reads seven kinds in one fixed order,
//! keeps one reader of its own.
//!
//! # The pointer
//!
//! A press of the primary pointer button - the left mouse button, or a
//! touch - on a button that is not [`InteractionDisabled`] inserts Bevy's
//! [`Pressed`] ([`press_on_primary_down`]). The press bubbles up from
//! whatever was hit, a button's label included, to the button, where it
//! stops. Releasing the primary button while the pointer is over a button
//! that is `Pressed` activates it ([`release_primary_presses`]), so a
//! misclick can be cancelled by dragging off before letting go, and a press
//! that began elsewhere and ends on a button does nothing. Any other release
//! ends the press without activating, and once no primary pointer is down,
//! no button stays `Pressed`.
//!
//! The release is matched against `Pressed` and this frame's hover map
//! rather than read from `bevy_picking`'s `Click` or `Release`. A click
//! needs the press and the release to land on the same *hit entity*, and a
//! button's label is a node of its own, so pressing on the label and
//! letting go over the button's padding would not be a click. And picking
//! sends a release to whatever was under the pointer the frame before,
//! where it sends a press to what is under it now, so a pointer that
//! arrives, presses and lets go within one frame would press one thing and
//! release another - a quick click on a slow frame, which a browser on a
//! weak machine produces, would do nothing. Both are pinned by tests.
//!
//! Only the primary button counts. A right or middle click does nothing at
//! all - not the pressed fill, not a cue - and stops at the button rather
//! than bubbling on. Picking never reaches a node that is `Display::None`
//! or hidden, so a hidden button cannot be clicked, even by a release whose
//! press it took before it was hidden. And the UI backend stops at the
//! first node under the pointer that carries no `Pickable` saying
//! otherwise, so the screens' full-screen dialog overlays are plain nodes,
//! and block every click to the screen beneath them for that reason alone.
//!
//! # Why Bevy's own button plugin is not added
//!
//! `bevy_ui_widgets` ships a `ButtonPlugin` of its own in `DefaultPlugins`,
//! and `crate::app` disables it. Its pointer half presses on any pointer
//! button, right and middle included, and activates on `Click`, both of
//! which the paragraphs above rule out. Its keyboard half triggers
//! `Activate` on whichever button holds Bevy's `InputFocus` when Enter or
//! Space goes down. The ring already activates on those keys, so that
//! would press every button twice; and it reads no `FocusYield`, so a
//! space typed into the Replay Library's search field, or Space bound
//! during a Settings key capture, would also press whichever button the
//! ring last focused. Disabling the plugin is what lets the ring keep
//! publishing `InputFocus` for a screen reader to follow (`ui::focus`'s
//! module docs) without that focus ever being pressed by a key the ring did
//! not read. The canary at the end of this module's tests pins both halves
//! of the upstream behaviour that makes this necessary.
//!
//! # Fills, the hover cue and the name
//!
//! [`Button`] requires only an `AccessibilityNode` with the button role.
//! [`UiButtonPlugin`] adds [`Hovered`] as a second required component, so
//! every button is hover-tracked by `bevy_picking` without a spawn site
//! having to remember it. A screen's fill pass reads a button's
//! [`PointerHighlight`] and hands its [`Highlight`] to
//! `presentation::button_background`, and `audio` reads `Hovered` for the
//! hover cue. The activation cue sounds when the button activates, which
//! for a click is its release, so a misclick dragged off makes no sound,
//! just as it does nothing. The ring draws an outline, not a fill, so a
//! keyboard or controller activation shows no pressed fill: it is over in
//! one frame, which nobody could see.
//!
//! The role is all `Button` brings for a screen reader. `bevy_ui` names its
//! own, deprecated, button after the text it holds, and no other, so
//! [`name_new_buttons`] does the same for these: each button is named once,
//! after the text it was spawned with, as the old button was. Keeping a
//! name current as its text changes is part of the screen-reader work
//! `docs/ideas/accessibility-goals.md` describes, not this.

use bevy::a11y::AccessibilitySystems;
use bevy::ecs::query::QueryData;
use bevy::ecs::system::SystemParam;
use bevy::input::InputSystems;
use bevy::input::touch::Touches;
use bevy::picking::PickingSystems;
use bevy::picking::events::PointerPress;
use bevy::picking::hover::{HoverMap, Hovered};
use bevy::picking::pointer::{PointerButton, PointerId};
use bevy::prelude::*;
use bevy::ui::{InteractionDisabled, Pressed};
use bevy::ui_widgets::{Activate, Button};

use crate::presentation::Highlight;

/// Where a pointer activates a button: after `bevy_picking`'s own
/// `PreUpdate` systems. The focus ring runs after it, so a frame's pointer
/// activations always come before the ring's.
#[derive(SystemSet, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) struct PointerActivation;

/// One activation of one button, by any input. Written only by
/// [`forward_activation`]; see the module docs.
#[derive(Message, Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct ButtonActivation {
    pub(crate) entity: Entity,
}

/// The pointer half of every button, and the activation message. Added once
/// by [`super::UiPlugin`], ahead of every screen, so that [`Hovered`] is a
/// required component of [`Button`] before the first button is spawned.
pub(crate) struct UiButtonPlugin;

impl UiButtonPlugin {
    /// Adds the plugin unless it is already there. Every plugin whose
    /// systems read [`ButtonActivation`] calls this, so that each also works
    /// on its own in a test's `App`, as `ui::focus`'s plugin does for the
    /// `bevy_input_focus` resources it reads.
    pub(super) fn require(app: &mut App) {
        if !app.is_plugin_added::<Self>() {
            app.add_plugins(Self);
        }
    }
}

impl Plugin for UiButtonPlugin {
    fn build(&self, app: &mut App) {
        // The two pointer-state resources normally come from `InputPlugin`;
        // initializing them here too keeps this plugin usable on a minimal
        // headless `App`, and `init_resource` never overwrites the
        // production copies.
        app.register_required_components::<Button, Hovered>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<Touches>()
            .add_message::<ButtonActivation>()
            .add_observer(press_on_primary_down)
            .add_observer(forward_activation)
            .add_systems(
                PreUpdate,
                release_primary_presses
                    .in_set(PointerActivation)
                    .after(InputSystems)
                    .after(PickingSystems::Last),
            )
            .add_systems(
                PostUpdate,
                name_new_buttons.before(AccessibilitySystems::Update),
            );
    }
}

/// A primary press on an enabled button marks it [`Pressed`]. Every pointer
/// press that reaches a button stops there, whichever button it was, so
/// none bubbles past a button to whatever lies under it.
fn press_on_primary_down(
    mut press: On<PointerPress>,
    buttons: Query<Has<InteractionDisabled>, With<Button>>,
    mut commands: Commands,
) {
    let Ok(disabled) = buttons.get(press.entity) else {
        return;
    };
    press.propagate(false);
    if press.button == PointerButton::Primary && !disabled {
        commands.entity(press.entity).insert(Pressed);
    }
}

/// Every button a pointer is pressing, with whether it is disabled.
type PressedButtons<'w, 's> =
    Query<'w, 's, (Entity, Has<InteractionDisabled>), (With<Pressed>, With<Button>)>;

/// Ends the presses a primary release ended this frame - the left mouse
/// button coming up, or a touch lifting - and activates each pressed button
/// that the releasing pointer is over *now*, provided it is still enabled.
/// The one place a pointer activates anything.
///
/// It reads the release from the device state and the button under the
/// pointer from `bevy_picking`'s hover map, rather than observing picking's
/// `Release` event, because that event is sent to whatever was under the
/// pointer the frame *before*, while a press is sent to what is under it
/// now. A pointer that reaches a button and presses and releases in the
/// same frame - a quick click on a slow frame - would otherwise press one
/// thing and release another, and do nothing. The hover map is this frame's
/// hit test, so it also never reaches a button that is hidden.
///
/// Once no primary pointer is down, no press is left: one dragged off its
/// button, or whose button was hidden while held, ends here without
/// activating, rather than wearing the pressed fill indefinitely and
/// activating on some later release that began elsewhere.
fn release_primary_presses(
    mouse: Res<ButtonInput<MouseButton>>,
    touches: Res<Touches>,
    hover_map: Option<Res<HoverMap>>,
    ancestors: Query<&ChildOf>,
    pressed: PressedButtons,
    mut commands: Commands,
) {
    if pressed.is_empty() {
        return;
    }
    let mut releasing: Vec<PointerId> = touches
        .iter_just_released()
        .map(|touch| PointerId::Touch(touch.id()))
        .collect();
    if mouse.just_released(MouseButton::Left) {
        releasing.push(PointerId::Mouse);
    }
    let under_a_release = |button: Entity| {
        hover_map.as_ref().is_some_and(|map| {
            releasing.iter().any(|pointer| {
                map.get(pointer).is_some_and(|hits| {
                    hits.keys().any(|hit| {
                        *hit == button || ancestors.iter_ancestors(*hit).any(|a| a == button)
                    })
                })
            })
        })
    };
    let still_held = mouse.pressed(MouseButton::Left) || touches.iter().next().is_some();
    for (button, disabled) in &pressed {
        let released_over = under_a_release(button);
        if released_over && !disabled {
            commands.trigger(Activate { entity: button });
        }
        if released_over || !still_held {
            commands.entity(button).remove::<Pressed>();
        }
    }
}

/// Turns every [`Activate`], from whichever input, into the
/// [`ButtonActivation`] the `Update` readers consume.
fn forward_activation(activate: On<Activate>, mut activations: MessageWriter<ButtonActivation>) {
    activations.write(ButtonActivation {
        entity: activate.entity,
    });
}

/// Names each new button after the text it was spawned with - every direct
/// child's that holds any, in order - for a screen reader. The old button's
/// name was its last such child alone, which made a binding row's name its
/// key, and that key text is still empty when the row spawns; joining them
/// names the row by its action instead. See the module docs for why the
/// name is not kept current.
fn name_new_buttons(
    buttons: Query<(Entity, &Children), Added<Button>>,
    texts: Query<&Text>,
    mut commands: Commands,
) {
    for (entity, children) in &buttons {
        let words: Vec<&str> = children
            .iter()
            .filter_map(|child| texts.get(child).ok())
            .map(|text| text.0.as_str())
            .filter(|words| !words.is_empty())
            .collect();
        if !words.is_empty() {
            commands
                .entity(entity)
                .insert(AccessibleLabel::new(words.join(" ")));
        }
    }
}

/// This frame's activations of the buttons carrying `C`, in the order they
/// happened - the reader each single-kind button handler uses.
#[derive(SystemParam)]
pub(crate) struct Activated<'w, 's, C: Component> {
    activations: MessageReader<'w, 's, ButtonActivation>,
    buttons: Query<'w, 's, &'static C>,
}

impl<C: Component> Activated<'_, '_, C> {
    /// Drains the unread activations, yielding the intent of each that
    /// landed on a button carrying `C`. Read it to the end: an activation
    /// left unread is seen again next frame.
    pub(crate) fn read(&mut self) -> impl Iterator<Item = &C> + '_ {
        let Self {
            activations,
            buttons,
        } = self;
        activations
            .read()
            .filter_map(|activation| buttons.get(activation.entity).ok())
    }
}

/// What the pointer is doing to a button, as a fill pass reads it. A
/// button with no [`Hovered`] - one spawned where [`UiButtonPlugin`] never
/// ran - is styled as unhovered rather than skipped.
#[derive(QueryData)]
pub(crate) struct PointerHighlight {
    pressed: Has<Pressed>,
    hovered: Option<&'static Hovered>,
}

impl PointerHighlightItem<'_, '_> {
    pub(crate) fn get(&self) -> Highlight {
        Highlight::new(self.pressed, self.hovered.is_some_and(Hovered::get))
    }
}

#[cfg(test)]
fn primary_window(app: &mut App) -> Entity {
    let world = app.world_mut();
    world
        .query_filtered::<Entity, With<bevy::window::PrimaryWindow>>()
        .single(world)
        .expect("real_layout_app has one primary window")
}

/// Moves the mouse to `position`, in logical pixels from the window's
/// top-left, and runs one frame: the `CursorMoved` message the ring and the
/// hover cue read, and the same event as the `WindowEvent` picking reads,
/// as winit writes both.
#[cfg(test)]
pub(crate) fn move_pointer(app: &mut App, position: Vec2) {
    use bevy::window::{CursorMoved, WindowEvent};

    let window = primary_window(app);
    let moved = CursorMoved {
        window,
        position,
        delta: None,
    };
    app.world_mut().write_message(moved.clone());
    app.world_mut()
        .write_message(WindowEvent::CursorMoved(moved));
    app.update();
}

/// Presses or releases one mouse button where the pointer is, for the next
/// frame, without running it - so a test can put a key press in the same
/// frame.
#[cfg(test)]
pub(crate) fn queue_mouse_button(
    app: &mut App,
    button: MouseButton,
    state: bevy::input::ButtonState,
) {
    use bevy::input::mouse::MouseButtonInput;
    use bevy::window::WindowEvent;

    let window = primary_window(app);
    let input = MouseButtonInput {
        button,
        state,
        window,
    };
    app.world_mut().write_message(input);
    app.world_mut()
        .write_message(WindowEvent::MouseButtonInput(input));
}

/// [`queue_mouse_button`], then one frame.
#[cfg(test)]
pub(crate) fn mouse_button(app: &mut App, button: MouseButton, state: bevy::input::ButtonState) {
    queue_mouse_button(app, button, state);
    app.update();
}

/// One touch event at `position`, and one frame.
#[cfg(test)]
pub(crate) fn touch(app: &mut App, phase: bevy::input::touch::TouchPhase, position: Vec2) {
    use bevy::input::touch::TouchInput;
    use bevy::window::WindowEvent;

    let window = primary_window(app);
    let input = TouchInput {
        phase,
        position,
        window,
        force: None,
        id: 0,
    };
    app.world_mut().write_message(input);
    app.world_mut()
        .write_message(WindowEvent::TouchInput(input));
    app.update();
}

/// The centre of `entity`'s node as laid out, in logical pixels from the
/// window's top-left.
#[cfg(test)]
pub(crate) fn centre_of(app: &App, entity: Entity) -> Vec2 {
    let world = app.world();
    let node = world
        .get::<ComputedNode>(entity)
        .expect("a button is a laid-out node");
    let transform = world
        .get::<bevy::ui::UiGlobalTransform>(entity)
        .expect("a laid-out node has a global transform");
    transform.translation * node.inverse_scale_factor
}

/// Moves the mouse onto the centre of `entity`, and runs one frame.
#[cfg(test)]
pub(crate) fn hover(app: &mut App, entity: Entity) {
    let centre = centre_of(app, entity);
    move_pointer(app, centre);
}

/// A whole left click on `entity`, as a player makes one: the pointer moves
/// onto the centre of the button, the left button goes down, and it comes
/// up again, one frame each. The activation lands in the last of the three.
#[cfg(test)]
pub(crate) fn click(app: &mut App, entity: Entity) {
    use bevy::input::ButtonState;

    hover(app, entity);
    mouse_button(app, MouseButton::Left, ButtonState::Pressed);
    mouse_button(app, MouseButton::Left, ButtonState::Released);
}

/// Every button picking could reach on `app` as it was last laid out: one
/// with a size, and visible.
#[cfg(test)]
pub(crate) fn reachable_buttons(app: &mut App) -> Vec<Entity> {
    let world = app.world_mut();
    world
        .query_filtered::<(Entity, &ComputedNode, &InheritedVisibility), With<Button>>()
        .iter(world)
        .filter(|(_, node, visibility)| node.size() != Vec2::ZERO && visibility.get())
        .map(|(entity, ..)| entity)
        .collect()
}

/// Moves the mouse onto the centre of each of `buttons` in turn and returns
/// those it found itself not hovering there - each covered by something
/// drawn above it that picking stops at. Nothing is pressed.
#[cfg(test)]
pub(crate) fn covered_at_their_centres(app: &mut App, buttons: &[Entity]) -> Vec<Entity> {
    buttons
        .iter()
        .copied()
        .filter(|button| {
            hover(app, *button);
            !app.world()
                .get::<Hovered>(*button)
                .expect("a button is hover-tracked")
                .get()
        })
        .collect()
}

/// Activates `entity` as every input does - Bevy's [`Activate`] on it - for
/// the next frame's readers, without running that frame, so a test can add
/// the other input it shares a frame with. For a handler's own tests, which
/// hold what activating an intent does; a test of how a button comes to be
/// activated presses it with [`click`], or with a key through the ring,
/// instead.
#[cfg(test)]
pub(crate) fn trigger_activation(app: &mut App, entity: Entity) {
    app.world_mut().trigger(Activate { entity });
}

/// Every [`ButtonActivation`] forwarded since [`recorded_activations`] was
/// last called, in order.
#[cfg(test)]
#[derive(Resource, Default)]
struct RecordedActivations(Vec<Entity>);

/// Makes `app` keep a list of every activation it forwards, for a test to
/// read with [`recorded_activations`] - the positive half of a test that
/// something else did not happen, and the plain record of what did.
#[cfg(test)]
pub(crate) fn record_activations(app: &mut App) {
    fn record(
        mut activations: MessageReader<ButtonActivation>,
        mut recorded: ResMut<RecordedActivations>,
    ) {
        recorded
            .0
            .extend(activations.read().map(|activation| activation.entity));
    }
    app.init_resource::<RecordedActivations>()
        .add_systems(Update, record);
}

/// The activations recorded since the last call, emptying the list.
#[cfg(test)]
pub(crate) fn recorded_activations(app: &mut App) -> Vec<Entity> {
    std::mem::take(&mut app.world_mut().resource_mut::<RecordedActivations>().0)
}

#[cfg(test)]
mod tests {
    use bevy::a11y::AccessibilityNode;
    use bevy::input::ButtonState;
    use bevy::input::touch::TouchPhase;

    use super::*;

    fn take(app: &mut App) -> Vec<Entity> {
        recorded_activations(app)
    }

    /// A point inside the button's padding, clear of its label.
    const PADDING: Vec2 = Vec2::new(380.0, 415.0);
    /// A point on the window, clear of the button.
    const OFF: Vec2 = Vec2::new(100.0, 100.0);

    /// A 300 by 80 button with a label, centred in the window, under real
    /// layout and real picking, with every activation recorded.
    fn button_app() -> (App, Entity, Entity) {
        let mut app = crate::ui::focus::real_layout_app();
        record_activations(&mut app);
        let (button, label) = spawn_button(&mut app);
        (app, button, label)
    }

    fn spawn_button(app: &mut App) -> (Entity, Entity) {
        let mut label = Entity::PLACEHOLDER;
        let mut button = Entity::PLACEHOLDER;
        app.world_mut()
            .spawn(Node {
                width: percent(100),
                height: percent(100),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(|root| {
                button = root
                    .spawn((
                        Button,
                        Node {
                            width: px(300),
                            height: px(80),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                    ))
                    .with_children(|button| {
                        label = button.spawn(Text::new("Press")).id();
                    })
                    .id();
            });
        app.update();
        (button, label)
    }

    fn highlight(app: &mut App, button: Entity) -> Highlight {
        let world = app.world_mut();
        world
            .query::<PointerHighlight>()
            .get(world, button)
            .expect("a button has a highlight")
            .get()
    }

    fn left(app: &mut App, state: ButtonState) {
        mouse_button(app, MouseButton::Left, state);
    }

    #[test]
    fn a_left_click_on_a_button_activates_it_on_the_release() {
        let (mut app, button, _) = button_app();
        let centre = centre_of(&app, button);
        assert_eq!(centre, Vec2::new(520.0, 450.0));
        assert_eq!(highlight(&mut app, button), Highlight::None);

        move_pointer(&mut app, centre);
        assert_eq!(highlight(&mut app, button), Highlight::Hovered);
        left(&mut app, ButtonState::Pressed);
        assert_eq!(highlight(&mut app, button), Highlight::Pressed);
        assert!(take(&mut app).is_empty(), "nothing activates on the press");
        left(&mut app, ButtonState::Released);
        assert_eq!(take(&mut app), [button]);
        assert_eq!(highlight(&mut app, button), Highlight::Hovered);
    }

    /// The release is matched against the press rather than read from
    /// picking's `Click`, which needs the same hit entity at both ends: a
    /// press on the label let go over the padding is one click on one
    /// button. The press reached the button by bubbling up from its label.
    #[test]
    fn a_press_on_the_label_released_over_the_padding_activates_the_button() {
        let (mut app, button, label) = button_app();
        let on_label = centre_of(&app, label);
        move_pointer(&mut app, on_label);
        left(&mut app, ButtonState::Pressed);
        assert!(app.world().get::<Pressed>(button).is_some());
        assert!(app.world().get::<Pressed>(label).is_none());

        move_pointer(&mut app, PADDING);
        assert!(app.world().get::<Hovered>(button).unwrap().get());
        left(&mut app, ButtonState::Released);
        assert_eq!(take(&mut app), [button]);
    }

    /// Dragging off before letting go cancels a misclick, which is why a
    /// click activates on the release; the press keeps its fill until
    /// then. Dragging back on before letting go makes it a click again.
    #[test]
    fn a_press_dragged_off_the_button_is_cancelled_and_one_dragged_back_is_not() {
        let (mut app, button, _) = button_app();
        let centre = centre_of(&app, button);
        move_pointer(&mut app, centre);
        left(&mut app, ButtonState::Pressed);
        move_pointer(&mut app, OFF);
        assert!(!app.world().get::<Hovered>(button).unwrap().get());
        assert_eq!(
            highlight(&mut app, button),
            Highlight::Pressed,
            "the press is held, so it keeps its fill"
        );
        left(&mut app, ButtonState::Released);
        assert!(take(&mut app).is_empty(), "the misclick is cancelled");
        assert_eq!(highlight(&mut app, button), Highlight::None);

        move_pointer(&mut app, centre);
        left(&mut app, ButtonState::Pressed);
        move_pointer(&mut app, OFF);
        move_pointer(&mut app, centre);
        left(&mut app, ButtonState::Released);
        assert_eq!(take(&mut app), [button]);
    }

    #[test]
    fn a_press_that_began_elsewhere_does_nothing_where_it_ends() {
        let (mut app, button, _) = button_app();
        let centre = centre_of(&app, button);
        move_pointer(&mut app, OFF);
        left(&mut app, ButtonState::Pressed);
        move_pointer(&mut app, centre);
        assert_eq!(highlight(&mut app, button), Highlight::Hovered);
        left(&mut app, ButtonState::Released);
        assert!(take(&mut app).is_empty());

        // The positive control: the same release after a press that began
        // on the button is a click.
        click(&mut app, button);
        assert_eq!(take(&mut app), [button]);
    }

    /// Only the primary button counts: a right or middle click shows no
    /// pressed fill and activates nothing, with the pointer over the
    /// button throughout, and a left click there afterwards does.
    #[test]
    fn a_right_or_middle_click_does_nothing_at_all() {
        let (mut app, button, _) = button_app();
        hover(&mut app, button);
        for other in [MouseButton::Right, MouseButton::Middle] {
            mouse_button(&mut app, other, ButtonState::Pressed);
            assert_eq!(highlight(&mut app, button), Highlight::Hovered, "{other:?}");
            mouse_button(&mut app, other, ButtonState::Released);
            assert!(take(&mut app).is_empty(), "{other:?}");
        }
        click(&mut app, button);
        assert_eq!(take(&mut app), [button]);
    }

    #[test]
    fn a_disabled_button_neither_takes_the_press_nor_activates() {
        let (mut app, button, _) = button_app();
        app.world_mut()
            .entity_mut(button)
            .insert(InteractionDisabled);
        hover(&mut app, button);
        left(&mut app, ButtonState::Pressed);
        assert!(app.world().get::<Pressed>(button).is_none());
        left(&mut app, ButtonState::Released);
        assert!(take(&mut app).is_empty());

        app.world_mut()
            .entity_mut(button)
            .remove::<InteractionDisabled>();
        click(&mut app, button);
        assert_eq!(take(&mut app), [button]);
    }

    /// Picking never reaches a button that is `Display::None` or hidden, so
    /// a click where it was drawn does nothing; shown again, it is clicked.
    #[test]
    fn a_hidden_button_cannot_be_clicked() {
        let (mut app, button, _) = button_app();
        let centre = centre_of(&app, button);
        let press_and_release = |app: &mut App| {
            move_pointer(app, OFF);
            move_pointer(app, centre);
            left(app, ButtonState::Pressed);
            left(app, ButtonState::Released);
        };

        app.world_mut().get_mut::<Node>(button).unwrap().display = Display::None;
        app.update();
        press_and_release(&mut app);
        assert!(take(&mut app).is_empty(), "Display::None");

        app.world_mut().get_mut::<Node>(button).unwrap().display = Display::Flex;
        app.world_mut()
            .entity_mut(button)
            .insert(Visibility::Hidden);
        app.update();
        press_and_release(&mut app);
        assert!(take(&mut app).is_empty(), "Visibility::Hidden");

        app.world_mut()
            .entity_mut(button)
            .insert(Visibility::Inherited);
        app.update();
        press_and_release(&mut app);
        assert_eq!(take(&mut app), [button]);
    }

    /// The screens' dialog overlays are plain nodes, and that is all it
    /// takes: a node with no `Pickable` stops picking, so the button under
    /// it is neither hovered nor clicked. Without the overlay it is both.
    #[test]
    fn a_plain_overlay_blocks_every_click_to_the_button_beneath_it() {
        let (mut app, button, _) = button_app();
        let overlay = app
            .world_mut()
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    width: percent(100),
                    height: percent(100),
                    ..default()
                },
                GlobalZIndex(100),
            ))
            .id();
        app.update();
        click(&mut app, button);
        assert!(!app.world().get::<Hovered>(button).unwrap().get());
        assert!(take(&mut app).is_empty());

        app.world_mut().entity_mut(overlay).despawn();
        app.update();
        click(&mut app, button);
        assert!(app.world().get::<Hovered>(button).unwrap().get());
        assert_eq!(take(&mut app), [button]);
    }

    /// A button hidden while held - a dialog's Confirm, held while Escape
    /// closes the dialog - neither activates on the release nor keeps its
    /// press, which would wear the pressed fill when the button is shown
    /// again and let a later release that began elsewhere activate it.
    /// (Picking itself sends that release to the hidden button, a frame
    /// late, which is one reason the release is not read from picking.)
    #[test]
    fn a_press_whose_button_is_hidden_while_held_is_ended_by_the_release() {
        let (mut app, button, _) = button_app();
        let centre = centre_of(&app, button);
        // The positive control: the same press and release, with the
        // button left on screen, is a click.
        click(&mut app, button);
        assert_eq!(take(&mut app), [button]);

        move_pointer(&mut app, centre);
        left(&mut app, ButtonState::Pressed);
        assert!(app.world().get::<Pressed>(button).is_some());
        app.world_mut().get_mut::<Node>(button).unwrap().display = Display::None;
        app.update();
        left(&mut app, ButtonState::Released);
        assert!(take(&mut app).is_empty());
        assert!(app.world().get::<Pressed>(button).is_none());

        app.world_mut().get_mut::<Node>(button).unwrap().display = Display::Flex;
        app.update();
        move_pointer(&mut app, OFF);
        left(&mut app, ButtonState::Pressed);
        move_pointer(&mut app, centre);
        left(&mut app, ButtonState::Released);
        assert!(take(&mut app).is_empty());
    }

    /// A pointer that arrives on a button, presses and lets go, all within
    /// one frame, clicks it: a quick click on a slow frame. Picking sends
    /// that release to what was under the pointer the frame before - here
    /// the empty window - which is why the release is matched against the
    /// hover map instead. Then the same, with two frames on the button, as
    /// a control.
    #[test]
    fn a_click_that_arrives_presses_and_releases_in_one_frame_activates() {
        use bevy::window::{CursorMoved, WindowEvent};

        let (mut app, button, _) = button_app();
        let centre = centre_of(&app, button);
        move_pointer(&mut app, OFF);
        let window = primary_window(&mut app);
        let moved = CursorMoved {
            window,
            position: centre,
            delta: None,
        };
        app.world_mut().write_message(moved.clone());
        app.world_mut()
            .write_message(WindowEvent::CursorMoved(moved));
        queue_mouse_button(&mut app, MouseButton::Left, ButtonState::Pressed);
        queue_mouse_button(&mut app, MouseButton::Left, ButtonState::Released);
        app.update();
        assert_eq!(take(&mut app), [button]);
        assert!(app.world().get::<Pressed>(button).is_none());

        click(&mut app, button);
        assert_eq!(take(&mut app), [button]);
    }

    /// A touch is the primary pointer too: it presses where it lands and
    /// activates where it lifts, and a touch that slides off first does
    /// not.
    #[test]
    fn a_touch_activates_its_button_where_it_lifts() {
        let (mut app, button, _) = button_app();
        let centre = centre_of(&app, button);
        touch(&mut app, TouchPhase::Started, centre);
        assert!(app.world().get::<Pressed>(button).is_some());
        assert!(take(&mut app).is_empty());
        touch(&mut app, TouchPhase::Ended, centre);
        assert_eq!(take(&mut app), [button]);
        assert!(app.world().get::<Pressed>(button).is_none());

        touch(&mut app, TouchPhase::Started, centre);
        touch(&mut app, TouchPhase::Moved, OFF);
        touch(&mut app, TouchPhase::Ended, OFF);
        assert!(take(&mut app).is_empty());
        assert!(app.world().get::<Pressed>(button).is_none());
    }

    /// Every button is named for a screen reader after the text it was
    /// spawned with - all of it, so a binding row whose key text starts
    /// empty is named by its action.
    #[test]
    fn a_button_is_named_after_the_text_it_was_spawned_with() {
        let (mut app, button, _) = button_app();
        let name = |app: &App, entity| {
            app.world()
                .get::<AccessibilityNode>(entity)
                .expect("Button requires an AccessibilityNode")
                .label()
                .map(str::to_owned)
        };
        assert_eq!(name(&app, button).as_deref(), Some("Press"));

        let row = app
            .world_mut()
            .spawn((Button, Node::default()))
            .with_children(|row| {
                row.spawn(Text::new("Move Up"));
                row.spawn(Text::new(""));
            })
            .id();
        app.update();
        assert_eq!(name(&app, row).as_deref(), Some("Move Up"));
    }

    #[derive(Resource, Default)]
    struct BevysActivations(Vec<Entity>);

    /// The canary for the decision `crate::app` makes when it disables
    /// `bevy_ui_widgets`' own `ButtonPlugin`, pinning the two upstream
    /// behaviours that make it necessary, on the same button under the
    /// same real picking. Bevy's plugin activates a button on a right
    /// click, which the game rules out; and it activates the button holding
    /// `InputFocus` on Enter, which the ring already does, so with it
    /// enabled every Enter would press twice and a Space typed into a text
    /// field would press the last button focused.
    ///
    /// If a Bevy upgrade makes this fail, re-examine whether the game's own
    /// pointer half and the disabled plugin are still needed, rather than
    /// only updating this test. The left click first is the positive
    /// control: it shows the fixture is driving Bevy's plugin at all.
    #[test]
    fn bevys_own_button_plugin_still_presses_on_any_pointer_button_and_on_enter() {
        use bevy::input::keyboard::{Key, KeyboardInput};
        use bevy::input_focus::{InputDispatchPlugin, InputFocus, InputFocusPlugin};
        use bevy::ui_widgets::ButtonPlugin;

        let mut app = crate::ui::focus::engine_ui_app();
        app.add_plugins((ButtonPlugin, InputFocusPlugin, InputDispatchPlugin))
            // The other two inputs `InputDispatchPlugin` dispatches.
            .add_message::<bevy::input::gamepad::GamepadButtonChangedEvent>()
            .add_message::<bevy::input::mouse::MouseWheel>()
            .init_resource::<BevysActivations>()
            .add_observer(
                |activate: On<Activate>, mut seen: ResMut<BevysActivations>| {
                    seen.0.push(activate.entity);
                },
            );
        let (button, _) = spawn_button(&mut app);
        let take = |app: &mut App| {
            std::mem::take(&mut app.world_mut().resource_mut::<BevysActivations>().0)
        };

        click(&mut app, button);
        assert_eq!(
            take(&mut app),
            [button],
            "Bevy's button plugin no longer activates on a left click under \
             this fixture, so the two findings below say nothing - rebuild \
             the fixture before trusting them"
        );

        mouse_button(&mut app, MouseButton::Right, ButtonState::Pressed);
        mouse_button(&mut app, MouseButton::Right, ButtonState::Released);
        assert_eq!(
            take(&mut app),
            [button],
            "Bevy's button plugin no longer activates on a right click: \
             re-examine whether `ui::button` still needs its own pointer half"
        );

        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(button, bevy::input_focus::FocusCause::Navigated);
        let window = primary_window(&mut app);
        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::Enter,
            logical_key: Key::Enter,
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window,
        });
        app.update();
        assert_eq!(
            take(&mut app),
            [button],
            "Bevy's button plugin no longer activates the focused button on \
             Enter: re-examine whether `crate::app` still needs to disable it"
        );
    }
}
