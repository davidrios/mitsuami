//! A private display for native tests.
//!
//! Tests don't open windows on the desktop: the runner starts its own
//! Broadway display (`gtk4-broadwayd`, GTK's in-memory display server), where
//! windows get exactly the size they ask for and nothing else competes for
//! focus. Desktop portals are off, so dialogs open there too and the
//! desktop's settings don't apply. `MITSUAMI_SHOW_WINDOWS=1` uses the
//! session's display instead.
//! While tests run, the display can be watched in a browser at the printed
//! address.

use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const DAEMON: &str = "gtk4-broadwayd";

/// Starts the display and points GDK at it. Call before GTK initializes,
/// while the process is still single-threaded.
pub(crate) fn start_private_display() {
    let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    // The daemon leaves its socket behind when it's killed. Clear dead ones
    // (nothing accepts connections on them) from earlier runs.
    for display in 100..920 {
        let socket = runtime_dir.join(format!("broadway{}.socket", display + 1));
        if socket.exists() && UnixStream::connect(&socket).is_err() {
            let _ = std::fs::remove_file(&socket);
        }
    }
    // Display numbers from the pid, so concurrent test processes don't meet.
    let first = 100 + std::process::id() % 800;
    for display in first..first + 20 {
        let mut command = Command::new(DAEMON);
        command
            .arg(format!(":{display}"))
            .args(["--address", "127.0.0.1"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        // SAFETY: prctl is async-signal-safe. The daemon goes when we do,
        // even if the runner exits without unwinding.
        unsafe {
            command.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                Ok(())
            });
        }
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(e) => panic!(
                "mitsuami-gtk: cannot start {DAEMON} ({e}). Native GTK tests run on a private Broadway \
                 display; install GTK's Broadway server, or set MITSUAMI_SHOW_WINDOWS=1 to use your display."
            ),
        };
        let socket = runtime_dir.join(format!("broadway{}.socket", display + 1));
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(5) {
            if UnixStream::connect(&socket).is_ok() {
                // SAFETY: nothing else runs yet (see above).
                unsafe {
                    std::env::set_var("GDK_BACKEND", "broadway");
                    std::env::set_var("BROADWAY_DISPLAY", format!(":{display}"));
                    // No desktop portals: file dialogs would open on the
                    // session's desktop, and its appearance settings (dark
                    // mode, fonts) would leak into tests.
                    std::env::set_var("GDK_DEBUG", "no-portals");
                }
                eprintln!("mitsuami-gtk: test windows are on http://127.0.0.1:{}", 8080 + display);
                // Dropping `child` leaves the daemon running until we exit.
                return;
            }
            // Exited: the display or its port is taken; try the next one.
            if child.try_wait().ok().flatten().is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = child.kill();
        let _ = child.wait();
    }
    panic!("mitsuami-gtk: could not start a private Broadway display");
}
