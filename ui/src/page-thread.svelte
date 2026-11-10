<!-- COMPS-15: a ticket's body as a thread — a page like every route's,
     placing tweet-thread by its bound mark (`plexus-ui:tweet-thread`) and
     nothing else. #/thread/:id: tweet-thread. -->
<script module>
  export const uses = [];
</script>

<script>
  import TweetThread from "plexus-ui:tweet-thread";
  let { id = null } = $props();
</script>

<section class="page page-thread">
  <TweetThread id={id}/>
</section>

<style>
  .page-thread { min-height: 100vh; color: var(--ink); background: var(--bg); font-family: var(--font-text); font-size: var(--text-size); }
</style>
