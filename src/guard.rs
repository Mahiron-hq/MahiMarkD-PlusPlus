//! Keeps panics from unwinding into Notepad++ or COM.

use std::panic::{AssertUnwindSafe, catch_unwind};

pub fn guarded<F: FnOnce()>(f: F) {
    let _ = catch_unwind(AssertUnwindSafe(f));
}

pub fn guarded_or<T, F: FnOnce() -> T>(fallback: T, f: F) -> T {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or(fallback)
}
