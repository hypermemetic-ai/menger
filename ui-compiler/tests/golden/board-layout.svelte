<script>
  prop("id");
  state(#{ wide: true });
  fn widen(state, ev) {
    state.wide = !state.wide;
    state
  }
</script>

<div class="board" data-id={id}>
  <header class="bar" on:dblclick={widen}>Board {id}</header>
  {#if state.wide}
    <slot name="main" id={id}/>
  {:else}
    <slot name="narrow" id={id} compact="yes"/>
  {/if}
  <slot name="side"/>
</div>
