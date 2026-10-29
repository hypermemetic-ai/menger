<script>
  prop("id");
  state(#{ q: "", open: true });
  let found = live(#{ snapshot: "tickets.find", params: #{ query: state.q, limit: 20 }, key: "id", debounce_ms: 80 });
  let body = live(#{ snapshot: "tickets.get", params: #{ id: id }, key: "id" });
  fn typed(state, ev) { state.q = ev.value; state }
  fn keyed(state, ev) { if ev.key == "Escape" { state.open = false; } state }
</script>

<div class="palette" on:window-keydown={keyed}>
  <input value={state.q} on:input={typed}/>
  <ul>{#each found as f (f.id)}<li>{f.title}</li>{/each}</ul>
  {@html markdown(state.q)}
</div>
