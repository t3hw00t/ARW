use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::Notify;

#[derive(Default)]
pub(crate) struct Singleflight {
    flights: Mutex<HashMap<String, Arc<FlightState>>>,
}

impl Singleflight {
    pub(crate) fn begin(&self, key: &str) -> FlightGuard<'_> {
        let mut map = self.flights.lock().expect("singleflight map lock poisoned");
        if let Some(state) = map.get(key) {
            state.add_ref();
            FlightGuard::new_follower(self, key.to_string(), Arc::clone(state))
        } else {
            let state = Arc::new(FlightState::new());
            map.insert(key.to_string(), Arc::clone(&state));
            FlightGuard::new_leader(self, key.to_string(), state)
        }
    }

    fn release(&self, key: &str, flight: &Arc<FlightState>) {
        let mut map = self.flights.lock().expect("singleflight map lock poisoned");
        if flight.release() == 0 {
            if let Some(existing) = map.get(key) {
                if Arc::ptr_eq(existing, flight) {
                    map.remove(key);
                }
            }
        }
    }
}

struct FlightState {
    notify: Notify,
    refs: AtomicUsize,
}

impl FlightState {
    fn new() -> Self {
        Self {
            notify: Notify::new(),
            refs: AtomicUsize::new(1),
        }
    }

    fn add_ref(&self) {
        self.refs.fetch_add(1, Ordering::Relaxed);
    }

    fn release(&self) -> usize {
        self.refs.fetch_sub(1, Ordering::AcqRel) - 1
    }

    async fn wait(&self) {
        self.notify.notified().await;
    }

    fn notify_waiters(&self) {
        self.notify.notify_waiters();
    }
}

pub(crate) struct FlightGuard<'a> {
    singleflight: &'a Singleflight,
    key: String,
    flight: Arc<FlightState>,
    notify_on_drop: bool,
    is_leader: bool,
}

impl<'a> FlightGuard<'a> {
    fn new_leader(singleflight: &'a Singleflight, key: String, flight: Arc<FlightState>) -> Self {
        Self {
            singleflight,
            key,
            flight,
            notify_on_drop: true,
            is_leader: true,
        }
    }

    fn new_follower(singleflight: &'a Singleflight, key: String, flight: Arc<FlightState>) -> Self {
        Self {
            singleflight,
            key,
            flight,
            notify_on_drop: false,
            is_leader: false,
        }
    }

    pub(crate) fn is_leader(&self) -> bool {
        self.is_leader
    }

    pub(crate) async fn wait(&self) {
        self.flight.wait().await;
    }

    pub(crate) fn notify_waiters(&mut self) {
        self.flight.notify_waiters();
        self.notify_on_drop = false;
    }
}

impl Drop for FlightGuard<'_> {
    fn drop(&mut self) {
        if self.notify_on_drop {
            self.flight.notify_waiters();
            self.notify_on_drop = false;
        }
        self.singleflight.release(&self.key, &self.flight);
    }
}

#[cfg(test)]
mod loom_tests {
    use super::*;
    use loom::sync::atomic::{AtomicUsize, Ordering};
    use loom::sync::{Arc, Condvar, Mutex};
    use loom::thread;

    // A minimal model of singleflight semantics using loom primitives.
    // It verifies that followers waiting on the leader's completion are released
    // and that refcounts are decremented without races.
    #[test]
    fn singleflight_no_deadlock() {
        loom::model(|| {
            struct Flight {
                refs: AtomicUsize,
                done: Mutex<bool>,
                cv: Condvar,
            }
            impl Flight {
                fn new() -> Self {
                    Self {
                        refs: AtomicUsize::new(1),
                        done: Mutex::new(false),
                        cv: Condvar::new(),
                    }
                }
                fn add_ref(&self) {
                    self.refs.fetch_add(1, Ordering::Relaxed);
                }
                fn release(&self) {
                    self.refs.fetch_sub(1, Ordering::AcqRel);
                }
                fn mark_done(&self) {
                    let mut d = self.done.lock().unwrap();
                    *d = true;
                    self.cv.notify_all();
                }
                fn wait(&self) {
                    let mut d = self.done.lock().unwrap();
                    while !*d {
                        d = self.cv.wait(d).unwrap();
                    }
                }
            }

            let f = Arc::new(Flight::new());
            let f1 = f.clone();
            let f2 = f.clone();

            // Follower waits
            let t1 = thread::spawn(move || {
                f1.add_ref();
                f1.wait();
                f1.release();
            });

            // Leader marks done
            let t2 = thread::spawn(move || {
                // simulate work
                f2.mark_done();
                f2.release();
            });

            t1.join().unwrap();
            t2.join().unwrap();
        });
    }
}
