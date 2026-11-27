//! Small helpers shared by unit tests across the crate.
//!
//! These exist so tests can drive real Bevy systems - rather than calling
//! internal functions directly - without every module repeating the same
//! boilerplate.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use bevy::ecs::schedule::SingleThreadedExecutor;
use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::log::Level;
use bevy::log::tracing::field::{Field, Visit};
use bevy::log::tracing::{self, Event, Subscriber};
use bevy::log::tracing_subscriber::layer::{Context, Layer, SubscriberExt};
use bevy::log::tracing_subscriber::registry::Registry;
use bevy::prelude::*;

/// Advances the app's `Time` by `seconds` and runs exactly one frame.
///
/// Tests use this instead of a bare `app.update()` so that time-dependent
/// behaviour (holds, countdowns, fades) is driven deterministically rather than
/// by the wall clock.
pub(crate) fn advance_test_app(app: &mut App, seconds: f32) {
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::from_secs_f32(seconds));
    app.update();
}

/// Writes a raw [`KeyboardInput`] message and runs one frame.
///
/// Raw messages are used rather than `ButtonInput` because some features - the
/// home marimba in particular - deliberately react to key *repeats*, which
/// `ButtonInput` collapses away.
pub(crate) fn send_keyboard_event(
    app: &mut App,
    key_code: KeyCode,
    state: ButtonState,
    repeat: bool,
) {
    app.world_mut().write_message(KeyboardInput {
        key_code,
        logical_key: Key::Character("a".into()),
        state,
        text: None,
        repeat,
        window: Entity::PLACEHOLDER,
    });
    app.update();
}

/// Runs `frames` frames of `app` with a `tracing` subscriber that records
/// every event, and returns what was logged, in order.
///
/// This is how a test proves a log line is actually *emitted* rather than
/// that the code path producing it ran. Two details make it work, and both
/// are easy to get wrong:
///
/// * The subscriber is scoped ([`tracing::subscriber::with_default`]), never
///   global. `tracing`'s global default can be set once per process and
///   Bevy's `LogPlugin` is the thing that sets it, so a test reaching for
///   `set_global_default` would capture nothing the moment any other test in
///   the same binary got there first.
/// * A scoped subscriber is *thread-local*, and Bevy hands schedule systems
///   to the task pool, so a system's event is emitted on a thread that cannot
///   see it. Both schedules this runs are therefore forced onto the calling
///   thread with `SingleThreadedExecutor`. Without that the capture is
///   silently empty, which reads exactly like a system that did not log.
pub(crate) fn capture_log_events(app: &mut App, frames: usize) -> Vec<CapturedLog> {
    app.edit_schedule(Startup, |schedule| {
        schedule.set_executor(SingleThreadedExecutor::default());
    });
    app.edit_schedule(Update, |schedule| {
        schedule.set_executor(SingleThreadedExecutor::default());
    });

    let captured = Arc::new(Mutex::new(Vec::new()));
    let subscriber = Registry::default().with(CaptureLayer(Arc::clone(&captured)));
    tracing::subscriber::with_default(subscriber, || {
        for _ in 0..frames {
            app.update();
        }
    });

    let captured = captured.lock().expect("capture sink was not poisoned");
    captured.clone()
}

/// One event [`capture_log_events`] saw.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CapturedLog {
    pub(crate) level: Level,
    pub(crate) message: String,
}

struct CaptureLayer(Arc<Mutex<Vec<CapturedLog>>>);

impl<S: Subscriber> Layer<S> for CaptureLayer {
    fn on_event(&self, event: &Event<'_>, _context: Context<'_, S>) {
        let mut message = MessageVisitor(String::new());
        event.record(&mut message);
        self.0
            .lock()
            .expect("capture sink was not poisoned")
            .push(CapturedLog {
                level: *event.metadata().level(),
                message: message.0,
            });
    }
}

/// Pulls out the `message` field - what `warn!("{detail}")` and friends put
/// the formatted text in - and ignores every other structured field, since
/// nothing in this crate logs with any.
struct MessageVisitor(String);

impl Visit for MessageVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.0 = format!("{value:?}");
        }
    }
}
