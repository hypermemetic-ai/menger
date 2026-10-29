//! UI-6's testing surface: the signal runtime.

use super::*;
use std::cell::RefCell;
use std::rc::Rc;

/// Three bindings over two signals — `a` reads `x`, `b` reads `y`, `c`
/// reads both; setting one runs exactly the bindings that read it, once
/// each; setting it to the value it holds runs none; a disposed binding
/// never runs again.
#[test]
fn a_binding_re_evaluates_only_when_a_signal_it_read_changes() {
    let rt = Runtime::new();
    let (x, y) = (rt.signal(1i64), rt.signal(10i64));
    let ran: Rc<RefCell<Vec<&'static str>>> = Rc::default();

    let (log, xa) = (ran.clone(), x.clone());
    let a = rt.bind(move || {
        let _ = xa.get();
        log.borrow_mut().push("a");
    });
    let (log, yb) = (ran.clone(), y.clone());
    let _b = rt.bind(move || {
        let _ = yb.get();
        log.borrow_mut().push("b");
    });
    let (log, xc, yc) = (ran.clone(), x.clone(), y.clone());
    let seen: Rc<RefCell<Vec<i64>>> = Rc::default();
    let sum = seen.clone();
    let _c = rt.bind(move || {
        sum.borrow_mut().push(xc.get() + yc.get());
        log.borrow_mut().push("c");
    });
    assert_eq!(*ran.borrow(), ["a", "b", "c"], "each binding runs once when made");

    ran.borrow_mut().clear();
    x.set(2);
    let mut got = ran.borrow().clone();
    got.sort();
    assert_eq!(got, ["a", "c"], "x read by a and c: exactly those ran, once each");
    assert_eq!(*seen.borrow().last().unwrap(), 12, "c saw the new x");

    ran.borrow_mut().clear();
    y.set(20);
    let mut got = ran.borrow().clone();
    got.sort();
    assert_eq!(got, ["b", "c"], "y read by b and c: exactly those ran");

    ran.borrow_mut().clear();
    y.set(20);
    assert!(ran.borrow().is_empty(), "the same value again: nothing ran, got {:?}", ran.borrow());

    ran.borrow_mut().clear();
    rt.batch(|| {
        x.set(3);
        y.set(30);
    });
    let mut got = ran.borrow().clone();
    got.sort();
    assert_eq!(got, ["a", "b", "c"], "a batch of both: each reader once, c not twice");

    ran.borrow_mut().clear();
    rt.dispose(a);
    x.set(4);
    assert_eq!(*ran.borrow(), ["c"], "a disposed: only c ran");
    assert_eq!(x.peek(), 4);

    // A conditional read: `d` reads `y` only while `flag` holds. Each run
    // tracks afresh, so `y` wakes it only then.
    let flag = rt.signal(true);
    let runs: Rc<RefCell<u32>> = Rc::default();
    let (f, yd, n) = (flag.clone(), y.clone(), runs.clone());
    let _d = rt.bind(move || {
        *n.borrow_mut() += 1;
        if f.get() {
            let _ = yd.get();
        }
    });
    assert_eq!(*runs.borrow(), 1);
    y.set(31);
    assert_eq!(*runs.borrow(), 2, "flag holds: y read, y woke d");
    flag.set(false);
    assert_eq!(*runs.borrow(), 3, "flag moved");
    y.set(32);
    assert_eq!(*runs.borrow(), 3, "flag false: y was not read on the last run, d did not run");
    flag.set(true);
    y.set(33);
    assert_eq!(*runs.borrow(), 5, "flag again: y read again, so y wakes it");

    // The evaluation counter counts every binding run.
    let before = rt.runs();
    x.set(5);
    assert_eq!(rt.runs() - before, 1, "x is read by c alone now");
}
