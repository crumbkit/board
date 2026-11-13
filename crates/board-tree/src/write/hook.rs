//! A test seam: code that runs on the put's thread after its commit is
//! staged and before HEAD moves, so a test can move HEAD at that moment.
//! Built only with the `testing` feature.

use std::cell::RefCell;

type Hook = Box<dyn FnMut(usize)>;

thread_local! {
    static HOOK: RefCell<Option<(Hook, usize)>> = const { RefCell::new(None) };
}

/// Removes the hook when dropped.
#[derive(Debug)]
pub struct HookGuard;

impl Drop for HookGuard {
    fn drop(&mut self) {
        HOOK.with(|hook| hook.borrow_mut().take());
    }
}

/// Runs `hook` on this thread before each compare-and-swap that moves HEAD,
/// given how many times it has run before (0 for the first attempt).
pub fn before_head_moves(hook: impl FnMut(usize) + 'static) -> HookGuard {
    HOOK.with(|slot| *slot.borrow_mut() = Some((Box::new(hook), 0)));
    HookGuard
}

pub(super) fn run() {
    // Taken out while it runs, so the hook may itself write.
    let Some((mut hook, attempt)) = HOOK.with(|slot| slot.borrow_mut().take()) else {
        return;
    };
    hook(attempt);
    HOOK.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            *slot = Some((hook, attempt + 1));
        }
    });
}
