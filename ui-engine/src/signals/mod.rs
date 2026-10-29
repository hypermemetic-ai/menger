//! UI-6: **the signal runtime** — no framework. A [`Signal<T>`] holds a
//! value; a binding ([`Runtime::bind`]) is a closure that is run once, its
//! reads of signals tracked, and run again exactly when one of the signals
//! it read last time is set to a different value. Nothing else re-runs.
//!
//! Each run tracks afresh, so a binding that reads `y` only when `x` holds
//! is not woken by `y` while `x` does not. A binding made while another
//! runs is its own (its reads are its own); bindings woken together run
//! oldest first, so an outer binding that disposes an inner one runs before
//! it. [`Runtime::untracked`] reads without tracking.
//!
//! Single-threaded (the page's one thread): handles are `Rc`s.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::marker::PhantomData;
use std::rc::Rc;

/// A binding's handle: [`Runtime::dispose`] stops it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BindingId(pub u64);

/// No binding: what [`Runtime::untracked`] pushes.
const NONE: u64 = 0;

struct Slot {
    value: Rc<dyn Any>,
    readers: BTreeSet<u64>,
}

struct Binding {
    /// Taken while it runs.
    f: Option<Box<dyn FnMut()>>,
    deps: BTreeSet<u64>,
}

#[derive(Default)]
struct Inner {
    signals: RefCell<Vec<Slot>>,
    bindings: RefCell<BTreeMap<u64, Binding>>,
    next_binding: Cell<u64>,
    stack: RefCell<Vec<u64>>,
    pending: RefCell<BTreeSet<u64>>,
    batch: Cell<u32>,
    flushing: Cell<bool>,
    runs: Cell<u64>,
}

/// The runtime: every signal and binding of one page (or one test).
#[derive(Clone, Default)]
pub struct Runtime {
    inner: Rc<Inner>,
}

impl std::fmt::Debug for Runtime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Runtime({} signals, {} bindings)", self.inner.signals.borrow().len(), self.inner.bindings.borrow().len())
    }
}

impl Runtime {
    /// A runtime with no signals.
    pub fn new() -> Self {
        Self::default()
    }

    /// A signal holding `value`.
    pub fn signal<T: Clone + PartialEq + 'static>(&self, value: T) -> Signal<T> {
        let mut signals = self.inner.signals.borrow_mut();
        signals.push(Slot { value: Rc::new(value), readers: BTreeSet::new() });
        Signal { id: signals.len() as u64 - 1, rt: self.clone(), _t: PhantomData }
    }

    /// Runs `f` now, tracking the signals it reads, and again whenever one
    /// of them changes.
    pub fn bind(&self, f: impl FnMut() + 'static) -> BindingId {
        let id = self.inner.next_binding.get() + 1;
        self.inner.next_binding.set(id);
        self.inner.bindings.borrow_mut().insert(id, Binding { f: Some(Box::new(f)), deps: BTreeSet::new() });
        self.run(id);
        BindingId(id)
    }

    /// Stops the binding: it never runs again, and its reads are forgotten.
    pub fn dispose(&self, id: BindingId) {
        let Some(b) = self.inner.bindings.borrow_mut().remove(&id.0) else { return };
        let mut signals = self.inner.signals.borrow_mut();
        for d in b.deps {
            signals[d as usize].readers.remove(&id.0);
        }
        self.inner.pending.borrow_mut().remove(&id.0);
    }

    /// Runs `f` with every set inside it batched: each binding runs once
    /// after `f`, however many of its signals moved.
    pub fn batch<R>(&self, f: impl FnOnce() -> R) -> R {
        self.inner.batch.set(self.inner.batch.get() + 1);
        let r = f();
        self.inner.batch.set(self.inner.batch.get() - 1);
        self.flush();
        r
    }

    /// Runs `f` reading signals without tracking them.
    pub fn untracked<R>(&self, f: impl FnOnce() -> R) -> R {
        self.inner.stack.borrow_mut().push(NONE);
        let r = f();
        self.inner.stack.borrow_mut().pop();
        r
    }

    /// How many times any binding has run — the tests' evaluation counter.
    pub fn runs(&self) -> u64 {
        self.inner.runs.get()
    }

    /// How many bindings are live.
    pub fn bindings(&self) -> usize {
        self.inner.bindings.borrow().len()
    }

    fn run(&self, id: u64) {
        let f = {
            let mut bindings = self.inner.bindings.borrow_mut();
            let Some(b) = bindings.get_mut(&id) else { return };
            let deps = std::mem::take(&mut b.deps);
            let f = b.f.take();
            drop(bindings);
            let mut signals = self.inner.signals.borrow_mut();
            for d in deps {
                signals[d as usize].readers.remove(&id);
            }
            f
        };
        let Some(mut f) = f else { return };
        self.inner.stack.borrow_mut().push(id);
        self.inner.runs.set(self.inner.runs.get() + 1);
        f();
        self.inner.stack.borrow_mut().pop();
        if let Some(b) = self.inner.bindings.borrow_mut().get_mut(&id) {
            b.f = Some(f);
        }
    }

    fn track(&self, signal: u64) {
        let top = self.inner.stack.borrow().last().copied().unwrap_or(NONE);
        if top == NONE {
            return;
        }
        if let Some(b) = self.inner.bindings.borrow_mut().get_mut(&top) {
            b.deps.insert(signal);
            self.inner.signals.borrow_mut()[signal as usize].readers.insert(top);
        }
    }

    fn wake(&self, signal: u64) {
        let readers = std::mem::take(&mut self.inner.signals.borrow_mut()[signal as usize].readers);
        self.inner.pending.borrow_mut().extend(readers);
        self.flush();
    }

    fn flush(&self) {
        if self.inner.batch.get() > 0 || self.inner.flushing.get() {
            return;
        }
        self.inner.flushing.set(true);
        loop {
            let next = self.inner.pending.borrow_mut().pop_first();
            match next {
                Some(id) => self.run(id),
                None => break,
            }
        }
        self.inner.flushing.set(false);
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
        self.rt.track(self.id);
        self.peek()
    }

    /// Its value, untracked.
    pub fn peek(&self) -> T {
        let v = self.rt.inner.signals.borrow()[self.id as usize].value.clone();
        v.downcast_ref::<T>().expect("a signal holds its own type").clone()
    }

    /// Sets it; the bindings that read it run again when the value differs.
    pub fn set(&self, value: T) {
        {
            let mut signals = self.rt.inner.signals.borrow_mut();
            let slot = &mut signals[self.id as usize];
            if slot.value.downcast_ref::<T>() == Some(&value) {
                return;
            }
            slot.value = Rc::new(value);
        }
        self.rt.wake(self.id);
    }

    /// Sets it to `f` of its value.
    pub fn update(&self, f: impl FnOnce(&mut T)) {
        let mut v = self.peek();
        f(&mut v);
        self.set(v);
    }
}

#[cfg(test)]
mod tests;
