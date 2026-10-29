fn typed(state, ev) { state.q = ev.value; state }
fn keyed(state, ev) { if ev.key == "Escape" { state.open = false; } state }
