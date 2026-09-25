use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};
use std::time::{Duration, Instant};

const STALL_TIMEOUT: Duration = Duration::from_secs(10);

/// Drives a test future on the current (main) thread.
///
/// Headless tests never actually wait: every await settles synchronously.
/// Native backends will plug their run loop in here.
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
        std::thread::yield_now();
    }
}
