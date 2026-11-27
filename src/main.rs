//! Thin binary entry point. All crate modules - the headless core and the
//! Bevy client alike - live in the `go_together` library crate (`src/lib.rs`);
//! this binary only starts the Bevy application.

fn main() {
    go_together::run();
}
