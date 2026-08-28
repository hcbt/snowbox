//! One booted-disk clone. The first hatch captures it; later hatches
//! clone that disk and cold-boot with a new identifier.

use std::sync::{Condvar, Mutex};

struct Gate {
    busy: bool,
}

struct BusyGuard;

impl Drop for BusyGuard {
    fn drop(&mut self) {
        GATE.lock().expect("ready gate").busy = false;
        CV.notify_all();
    }
}

static GATE: Mutex<Gate> = Mutex::new(Gate { busy: false });
static CV: Condvar = Condvar::new();

pub fn ensure(exists: impl Fn() -> bool, warm: impl Fn() -> Result<(), String>) {
    loop {
        if exists() {
            return;
        }
        let mut g = GATE.lock().expect("ready gate");
        if g.busy {
            drop(CV.wait(g).expect("ready gate"));
            continue;
        }
        g.busy = true;
        drop(g);
        let _guard = BusyGuard;
        if let Err(e) = warm() {
            eprintln!("ready snapshot: warm failed ({e})");
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::time::Duration;

    #[test]
    fn ensure_is_a_noop_when_the_snapshot_exists() {
        let warmed = AtomicUsize::new(0);
        ensure(
            || true,
            || {
                warmed.fetch_add(1, Ordering::SeqCst);
                Ok(())
            },
        );
        assert_eq!(warmed.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_waiter_does_not_start_a_second_warm() {
        let exists = AtomicBool::new(false);
        let warms = AtomicUsize::new(0);
        let started = AtomicBool::new(false);
        std::thread::scope(|s| {
            s.spawn(|| {
                ensure(
                    || exists.load(Ordering::SeqCst),
                    || {
                        warms.fetch_add(1, Ordering::SeqCst);
                        started.store(true, Ordering::SeqCst);
                        std::thread::sleep(Duration::from_millis(50));
                        exists.store(true, Ordering::SeqCst);
                        Ok(())
                    },
                );
            });
            while !started.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(1));
            }
            ensure(
                || exists.load(Ordering::SeqCst),
                || {
                    warms.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                },
            );
        });
        assert!(exists.load(Ordering::SeqCst));
        assert_eq!(warms.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn ensure_clears_busy_when_warm_panics() {
        let panicked = AtomicBool::new(false);
        let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            ensure(
                || false,
                || {
                    panicked.store(true, Ordering::SeqCst);
                    panic!("warm boom");
                },
            );
        }));
        assert!(caught.is_err());
        assert!(panicked.load(Ordering::SeqCst));

        let ran = AtomicBool::new(false);
        ensure(
            || ran.load(Ordering::SeqCst),
            || {
                ran.store(true, Ordering::SeqCst);
                Ok(())
            },
        );
        assert!(ran.load(Ordering::SeqCst));
    }
}
