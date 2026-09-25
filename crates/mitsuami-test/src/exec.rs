use std::cell::RefCell;
use std::future::Future;
use std::pin::pin;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};
use std::time::{Duration, Instant};

const STALL_TIMEOUT: Duration = Duration::from_secs(10);

thread_local! {
    /// Lets the platform run while the test waits (see [`set_idle`]).
    static IDLE: RefCell<Option<Rc<dyn Fn()>>> = const { RefCell::new(None) };
}

/// Sets what runs while a test future is pending: the running test's
/// backend catching up (GTK completes captures from its frame clock).
pub(crate) fn set_idle(idle: Option<Rc<dyn Fn()>>) {
    IDLE.with(|i| *i.borrow_mut() = idle);
}

/// Drives a test future on the current (main) thread.
///
/// Headless tests never actually wait: every await settles synchronously.
/// While a native test waits, its backend gets to run.
pub(crate) fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let mut cx = Context::from_waker(Waker::noop());
    let started = Instant::now();
    loop {
        if let Poll::Ready(value) = future.as_mut().poll(&mut cx) {
            return value;
        }
        if started.elapsed() > STALL_TIMEOUT {
            panic!("mitsuami-test: the test is waiting on something that never completes");
        }
        let idle = IDLE.with(|i| i.borrow().clone());
        match idle {
            Some(idle) => {
                idle();
                std::thread::sleep(Duration::from_millis(1));
            }
            None => std::thread::yield_now(),
        }
    }
}
