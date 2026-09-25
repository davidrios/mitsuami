//! Checks the real run loop end to end: a `sleep` timer fires, background
//! work wakes the UI thread, and closing the last window ends the app.
//! `cargo run -p mitsuami --example run_loop_smoke` should exit by itself
//! within a second; it hangs if timers or wake-ups are broken.

use std::time::{Duration, Instant};

use mitsuami::prelude::*;

fn main() {
    let started = Instant::now();
    App::new()
        .window("run loop smoke test", Size::new(320.0, 120.0), move || {
            let status = signal("waiting for a timer…".to_string());
            spawn_local(async move {
                sleep(Duration::from_millis(200)).await;
                status.set("waiting for a background thread…".into());
                let answer = spawn_blocking(|| {
                    std::thread::sleep(Duration::from_millis(100));
                    42
                })
                .await;
                status.set(format!("got {answer}, closing"));
                sleep(Duration::from_millis(100)).await;
                let ui = inject::<Ui>().expect("Ui in scope");
                for window in ui.windows() {
                    ui.destroy(window);
                }
            });
            Column::new().padding(20).child(Text::new(status))
        })
        .run();
    let elapsed = started.elapsed();
    println!("run loop smoke test passed in {elapsed:.2?}");
    assert!(elapsed >= Duration::from_millis(400), "timers fired too early");
}
