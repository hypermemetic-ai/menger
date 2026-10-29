fn toggle(state, ev) { state.expanded = !state.expanded; state }
fn rename(state, ev) { call("tickets.update", #{ id: ev.props.id, title: ev.value }); state }
