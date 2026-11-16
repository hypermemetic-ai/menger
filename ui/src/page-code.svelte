<!-- CODE-3: a repository at an address of its own — #/code/:owner/:name, …/:ref and …/:ref/:view (the owner's
     answer to CODE-1's `route`, 2026-11-15: "Yes: a route too, so a view is a link"). svelte/1. The page
     only places code-ide by its bound mark, filling the page: `repo` the route's owner and name, `ref` a
     branch, a tag or a commit's hash, `view` the address's last segment (`dir=…;open=…`, as the tag writes
     them; the router decodes each segment). code-ide keeps the address following what is on screen, in
     place. Marks: `[data-surface=page-code]`. `sheet` is handed on (the sheet draws it with no call). -->
<script module>
  export const uses = [];
</script>

<script>
  import CodeIde from "plexus-ui:code-ide";
  let { owner = "", name = "", ref = "", view = "", sheet = null } = $props();
  const repo = $derived(owner && name ? `${owner}/${name}` : "");
</script>

<section class="page page-code" data-surface="page-code">
  <CodeIde {repo} {ref} {view} page="true" {sheet}/>
</section>

<style>
  .page-code { min-height: 100vh; min-height: 100dvh; background: var(--bg); }
</style>
