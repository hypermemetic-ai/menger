//! UI-6: **the signal runtime** — no framework. A [`Signal<T>`] holds a
//! value; a binding ([`Runtime::bind`]) is a closure that is run once, its
//! reads of signals tracked, and run again exactly when one of the signals
//! it read last time is set to a different value. Nothing else re-runs.
//!
//! Single-threaded (the page's one thread): handles are `Rc`s.

use std::marker::PhantomData;

/// A binding's handle: [`Runtime::dispose`] stops it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BindingId(pub u64);

/// The runtime: every signal and binding of one page (or one test).
#[derive(Clone, Default)]
pub struct Runtime {
    _inner: std::rc::Rc<()>,
}

impl std::fmt::Debug for Runtime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Runtime")
    }
}

impl Runtime {
    /// A runtime with no signals.
    pub fn new() -> Self {
        Self::default()
    }

    /// A signal holding `value`.
    pub fn signal<T: Clone + PartialEq + 'static>(&self, value: T) -> Signal<T> {
        let _ = value;
        todo!("UI-6: Runtime::signal")
    }

    /// Runs `f` now, tracking the signals it reads, and again whenever one
    /// of them changes.
    pub fn bind(&self, f: impl FnMut() + 'static) -> BindingId {
        let _ = f;
        todo!("UI-6: Runtime::bind")
    }

    /// Stops the binding: it never runs again, and its reads are forgotten.
    pub fn dispose(&self, id: BindingId) {
        let _ = id;
        todo!("UI-6: Runtime::dispose")
    }

    /// Runs `f` with every set inside it batched: each binding runs once
    /// after `f`, however many of its signals moved.
    pub fn batch<R>(&self, f: impl FnOnce() -> R) -> R {
        let _ = f;
        todo!("UI-6: Runtime::batch")
    }
}

/// A value bindings read.
pub struct Signal<T> {
    id: u64,
    rt: Runtime,
    _t: PhantomData<T>,
}

impl<T> Clone for Signal<T> {
    fn clone(&self) -> Self {
        Self { id: self.id, rt: self.rt.clone(), _t: PhantomData }
    }
}

impl<T> std::fmt::Debug for Signal<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Signal({})", self.id)
    }
}

impl<T: Clone + PartialEq + 'static> Signal<T> {
    /// Its value — tracked: the binding running now reads it.
    pub fn get(&self) -> T {
        todo!("UI-6: Signal::get")
    }

    /// Its value, untracked.
    pub fn peek(&self) -> T {
        todo!("UI-6: Signal::peek")
    }

    /// Sets it; the bindings that read it run again when the value differs.
    pub fn set(&self, value: T) {
        let _ = (value, &self.rt);
        todo!("UI-6: Signal::set")
    }

    /// Sets it to `f` of its value.
    pub fn update(&self, f: impl FnOnce(&mut T)) {
        let _ = f;
        todo!("UI-6: Signal::update")
    }
}

#[cfg(test)]
mod tests;
