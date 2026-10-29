fn scrolled(state, ev) { state.seen = "scroll " + ev.scroll_top + "/" + ev.scroll_height + "/" + ev.client_height; state }
fn keyed(state, ev) { state.seen = "key " + ev.key; state }
fn ticked(state, ev) { state.seen = "checked " + ev.checked; state }
