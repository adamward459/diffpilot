use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

#[derive(Clone, Default)]
pub struct CancelToken(Arc<(Mutex<bool>, Condvar)>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        let (flag, wake) = &*self.0;
        *flag.lock().unwrap() = true;
        wake.notify_all();
    }

    pub fn is_cancelled(&self) -> bool {
        *self.0.0.lock().unwrap()
    }

    /// Sleeps up to `timeout` and wakes early on cancel. Returns whether the token is cancelled.
    pub fn wait(&self, timeout: Duration) -> bool {
        let (flag, wake) = &*self.0;
        let guard = flag.lock().unwrap();
        let (guard, _) = wake
            .wait_timeout_while(guard, timeout, |cancelled| !*cancelled)
            .unwrap();
        *guard
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn waiting_times_out_when_not_cancelled() {
        let token = CancelToken::new();
        assert!(!token.wait(Duration::from_millis(10)));
        assert!(!token.is_cancelled());
    }

    #[test]
    fn cancelling_wakes_a_waiter_early() {
        let token = CancelToken::new();
        let other = token.clone();
        let started = Instant::now();
        let handle = std::thread::spawn(move || other.wait(Duration::from_secs(10)));
        std::thread::sleep(Duration::from_millis(30));
        token.cancel();
        assert!(handle.join().unwrap());
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}
