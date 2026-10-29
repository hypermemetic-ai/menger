fn toggle(state, ev) { state.expanded = !state.expanded; state }
fn rename(state, ev) { call("tickets.update", #{ id: id, title: ev.value }); state }
