<!-- SURFACE-9: who I am (#/me), svelte/1 — the connection's identity
     (`session.whoami`: principal, name, provider, tenant) and the door's
     (`auth.whoami`: the memberships as roles, the agent, a ceiling), the
     providers (`auth.providers`, the one signed in with marked;
     `session.providers` beside them), my teams and every grant I hold
     (`members.mine`: `me`, then `team { team }` and `grant { grant }` rows:
     role, scope, by whom, since), "who may…" — `auth.requires { method }`
     asked for the method entered (`bind:value`), answered with its grant,
     the roles that hold it and where I hold one of them, a method there is
     none of refused in place, the methods asked before kept as "recent" —
     and the way out: the form, posted to the shell's door, /logout — the
     page runs on the shell's cookie, and the door ends it (`session.logout`
     is the door's, not reached from here).
     STUDIO-12: the page's title is page-me's (rhx). -->
<script module>
  export const uses = ["session.whoami", "session.providers", "auth.whoami", "auth.providers", "auth.requires", "members.mine"];
</script>

<script>
  import { bus, live } from "plexus-ui/host";
  // SURFACE-10: the sheet holds the loading state by its mark (data-sheet="loading").
  let { sheet = null } = $props();

  const b = bus();
  const who = live({ snapshot: "session.whoami", params: {}, key: "principal", on: {} });
  const door = live({ snapshot: "auth.whoami", params: {}, key: "principal", on: {} });
  const mine = live({ snapshot: "members.mine", params: {}, key: "name", on: {} });
  const sessions = live({ snapshot: "session.providers", params: {}, key: "name", on: {} });
  const providers = live({ snapshot: "auth.providers", params: {}, key: "name", on: {} });

  let draft = $state("");
  let method = $state(null);
  let recent = $state([]);
  const req = live({ snapshot: "auth.requires", params: () => ({ method }), key: "method", when: () => method != null, on: {} });

  const me = $derived($who.rows[0]);
  const memberships = $derived($door.rows[0]?.memberships ?? []);
  const grants = $derived($mine.rows.filter((r) => r.role != null));
  const teams = $derived($mine.rows.filter((r) => r.members != null));
  const everywhere = $derived($mine.rows.some((r) => r.admin_everywhere === true));
  // An answer is drawn only for the request standing now: a method the wire
  // refused keeps the last rows, which are not this method's.
  const answer = $derived(method != null && $req.status === "live" ? $req.rows.find((r) => r.method === method) ?? null : null);
  const held = $derived(answer ? memberships.filter((m) => answer.roles.includes(m.role)) : []);
  const refused = $derived(method != null && ($req.status === "error" || $req.status === "refused"));
  const since = (t) => (t ? new Date(t * 1000).toLocaleDateString(undefined, { month: "short", day: "2-digit" }) : "from the start");
  const ceiling = (c) => (typeof c === "string" ? c : `${c.role}@${c.scope}`);

  function asked(m) {
    m = (m ?? "").trim();
    if (!m) return;
    if (method != null && method !== m && !recent.includes(method)) recent = [...recent, method].slice(-6);
    draft = m;
    method = m;
  }
  // Sign out is the shell's door alone: a page at /app/ runs on the shell's
  // cookie, and POST /logout ends that session, revokes it at its backend and
  // walks the provider's own sign-out. `session.logout` is the door's, not
  // reached from here (as `auth.logout`): sent first over the socket, it would
  // leave /logout no session to end.
  // A team's own grants (a grant whose principal names the team).
  const teamGrants = (t) => grants.filter((g) => typeof g.principal === "string" && g.principal.endsWith(":" + t.name));
</script>

<section class="me-page" data-surface="me-page">
  <div class="page-head">
    <span class="sub">your session, your grants, who may what</span>
    {#if $who.status !== "live"}<span class="state" data-loading={$who.status}>{$who.status}</span>{/if}
  </div>
  <div class="me">
    <div>
      <section class="panel" data-method="session.whoami">
        <h2>signed in</h2>
        {#if me}
          <dl>
            <dt>principal</dt><dd class="principal" data-field="principal" title={me.principal}>{me.principal}</dd>
            <dt>name</dt><dd data-field="name">{me.name}</dd>
            <dt>provider</dt><dd data-field="provider">{me.provider}</dd>
            <dt>tenant</dt><dd data-field="tenant">{me.tenant ?? "none"}</dd>
            <dt>roles</dt><dd data-field="roles">{#each memberships as m (m.role + "@" + m.scope)}<span class="role" data-role={m.role} title={"on " + m.scope}>{m.role}</span> {/each}</dd>
            <dt>agent</dt><dd data-field="agent">{$door.rows[0]?.agent ?? "none"}</dd>
            <dt>ceiling</dt><dd class="muted" data-field="ceiling">{($door.rows[0]?.ceiling ?? []).map(ceiling).join(", ") || "none"}</dd>
          </dl>
        {:else if $who.status === "pending"}
          <dl><dt>principal</dt><dd data-loading="">key:0000000000</dd></dl>
        {/if}
        {#if $who.status === "error" || $who.status === "refused"}<p class="pad" data-refused="">refused: {$who.message}</p>{/if}
        <form class="who-acts" method="post" action="/logout">
          <button class="btn warn" type="submit" data-action="session.logout">sign out</button>
        </form>
      </section>
      <section class="panel" data-method="session.providers" data-field="providers">
        <h2>providers</h2>
        <div class="prov">
          {#each providers.rows as p (p.name)}<span class="pill" data-id={p.name} data-current={me?.provider === p.name} title={p.kind}>{p.name}</span>{/each}
          {#each sessions.rows as p (p.name)}<span class="pill" data-session-provider={p.name}>{p.name}</span>{/each}
          {#if $providers.status === "live" && providers.rows.length === 0 && sessions.rows.length === 0}<span data-empty="">No provider named.</span>{/if}
        </div>
      </section>
      <section class="panel" data-method="members.mine" data-field="teams">
        <h2>teams <span class="m">{teams.length}</span></h2>
        {#each teams as t (t.name)}
          <div class="team" data-id={t.name}><b data-field="team">{t.name}</b>{#each teamGrants(t) as g (g.principal + "@" + g.scope)}<span class="role" data-role={g.role} title={"on " + g.scope}>{g.role}</span>{/each}<span class="muted"><span data-field="members">{t.members}</span> members · {t.org}</span></div>
        {/each}
        {#if $mine.status === "live" && teams.length === 0}<p class="pad" data-empty="">In no team.</p>{/if}
      </section>
    </div>
    <div>
      <section class="panel" data-method="members.mine" data-field="grants">
        <h2>grants <span class="m">{grants.length}</span>{#if everywhere}<span class="yes" data-field="admin">admin everywhere</span>{/if}</h2>
        <div class="th"><span>role</span><span>scope</span><span>held by</span><span>granted by</span><span>since</span></div>
        {#each grants as g ((g.principal ?? "") + "|" + g.scope)}
          <div class="tr" data-id={g.role + "@" + g.scope} data-principal={g.principal}>
            <span><span class="role" data-role={g.role} data-field="role">{g.role}</span></span>
            <span><code data-field="scope">{g.scope}</code></span>
            <span class="muted" data-field="principal" title={g.principal}>{g.principal === me?.principal ? "you" : g.principal}</span>
            <span class="muted" data-field="by" title={g.granted_by}>{g.granted_by ?? ""}</span>
            <span class="muted" data-field="since">{since(g.at)}</span>
          </div>
        {/each}
        {#if sheet === "loading" || $mine.status === "pending"}<div class="tr"><span data-loading="">admin</span></div>{/if}
        {#if $mine.status === "live" && grants.length === 0}<p class="pad" data-empty="">No grant: you hold nothing yet.</p>{/if}
      </section>
      <section class="panel" data-method="auth.requires">
        <h2>who may…</h2>
        <div class="req" data-form="auth.requires">
          <div class="ask">
            <input class="field" aria-label="a method" placeholder="a method — tickets.set_status" spellcheck="false" autocomplete="off"
                   bind:value={draft} aria-invalid={refused} onkeydown={(e) => { if (e.key === "Enter") asked(e.currentTarget.value); }}/>
            <button class="btn primary" type="button" data-action="auth.requires" onclick={() => asked(draft)}>ask</button>
          </div>
          {#if recent.length > 0}
            <div class="sugg">recent {#each recent as m (m)}<button type="button" data-sugg={m} onclick={() => asked(m)}>{m}</button>{/each}</div>
          {/if}
          {#if answer}
            <div class="answer" data-answer="auth.requires">
              <h3>who may <code>{answer.method}</code></h3>
              <dl>
                <dt>grant</dt><dd><code data-field="grant">{answer.grant}</code></dd>
                <dt>roles that hold it</dt><dd data-field="roles">{#each answer.roles as x (x)}<span class="role" data-role={x}>{x}</span> {/each}</dd>
                <dt>you</dt>
                <dd data-field="may">{#if held.length > 0}<span class="yes">may</span>, {held.map((m) => `on ${m.scope} as ${m.role}`).join("; ")}{#if !held.some((m) => m.scope === "*")} — <span class="no">not elsewhere</span>{/if}{:else}<span class="no">may not</span> — none of your grants holds it{/if}</dd>
              </dl>
            </div>
          {:else if method != null && ($req.status === "pending" || $req.status === "resyncing")}
            <div class="answer"><span data-loading="">asking who may</span></div>
          {/if}
          {#if refused}<p data-refused="" role="alert">refused: {$req.message}</p>{/if}
        </div>
      </section>
    </div>
  </div>
</section>

<style>
  .me-page { max-width: 1180px; margin: 0 auto; padding: 18px 24px 40px; color: var(--ink); font-family: var(--font-text); font-size: var(--text-size); }
  .page-head { display: flex; align-items: baseline; gap: 10px; margin: 0 0 14px; }
  .sub { color: var(--muted); font-size: 12px; }
  .state { margin-left: auto; font-size: 11px; color: var(--muted); }
  .me { display: grid; grid-template-columns: 380px minmax(0, 1fr); gap: 20px; align-items: start; }
  .panel { margin-bottom: 14px; overflow: hidden; background: var(--panel); border: var(--border) solid var(--line); border-radius: var(--radius-lg); }
  h2 { display: flex; gap: 8px; align-items: baseline; margin: 0; padding: 8px 12px; font-size: 11px; font-weight: var(--weight-medium); color: var(--muted); text-transform: uppercase; letter-spacing: .06em; border-bottom: var(--border) solid var(--line); }
  h2 .m { margin-left: auto; text-transform: none; letter-spacing: 0; }
  h2 .yes { text-transform: none; letter-spacing: 0; }
  dl { display: grid; grid-template-columns: 96px minmax(0, 1fr); gap: 6px 10px; margin: 0; padding: 10px 12px; }
  dt { color: var(--muted); font-size: 12px; }
  dd { margin: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .principal { color: var(--accent); }
  .muted { color: var(--muted); }
  .pad { margin: 8px 12px; }
  .who-acts { display: flex; gap: 8px; align-items: center; margin: 0; padding: 0 12px 12px; }
  .prov { display: flex; flex-wrap: wrap; gap: 6px; padding: 10px 12px; }
  .pill { display: inline-block; padding: 0 7px; font-size: 11px; line-height: 18px; color: var(--chip-fg); background: var(--chip-bg); border: var(--border) solid var(--chip-line); border-radius: var(--radius-pill); }
  .pill[data-current=true] { color: var(--accent); border-color: var(--accent); background: var(--sel); }
  .team { display: flex; gap: 8px; align-items: center; padding: 8px 12px; border-bottom: var(--border) solid var(--line); transition: background-color var(--motion-fast) var(--ease); }
  .team:last-child { border-bottom: 0; }
  .team:hover, .team:global([data-sheet~=hover]) { background: var(--hover); }
  .team .muted { margin-left: auto; font-size: 12px; }
  .role { display: inline-block; padding: 0 7px; font-size: 11px; line-height: 18px; border-radius: var(--radius-md); color: var(--st-ready-fg); background: var(--st-ready-bg); }
  .role[data-role=admin] { color: var(--st-coding-fg); background: var(--st-coding-bg); }
  .role[data-role=viewer] { color: var(--st-draft-fg); background: var(--st-draft-bg); }
  .role[data-role=deployer] { color: var(--st-in-review-fg); background: var(--st-in-review-bg); }
  .th, .tr { display: grid; grid-template-columns: 90px minmax(0, 1fr) minmax(0, 1fr) minmax(0, 1fr) 110px; gap: 0 10px; align-items: center; padding: 0 10px; }
  .th { padding-top: 6px; padding-bottom: 6px; font-size: 11px; font-weight: var(--weight-medium); color: var(--muted); border-bottom: var(--border) solid var(--line); }
  .tr { min-height: var(--pitch); border-bottom: var(--border) solid var(--line); transition: background-color var(--motion-fast) var(--ease); }
  .tr:last-child { border-bottom: 0; }
  .tr:hover, .tr:global([data-sheet~=hover]) { background: var(--hover); }
  .tr > span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  code { font-family: var(--font-text); font-size: 12px; }
  .req { padding: 12px; }
  .ask { display: flex; gap: 8px; }
  .field { flex: 1; padding: 5px 10px; color: var(--ink); background: var(--bg); border: var(--border) solid var(--line); border-radius: var(--radius); transition: border-color var(--motion-fast) var(--ease); }
  .field:hover, .field:global([data-sheet~=hover]) { border-color: var(--muted); }
  .field:focus-visible, .field:global([data-sheet~=focus]) { outline: none; border-color: var(--focus); box-shadow: 0 0 0 2px var(--sel); }
  .field[aria-invalid=true] { border-color: var(--warn); }
  .btn { padding: 3px 10px; font-size: 12px; color: var(--ink); background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius); cursor: pointer; transition: border-color var(--motion-fast) var(--ease), background-color var(--motion-fast) var(--ease); }
  .btn:hover:not(:disabled), .btn:global([data-sheet~=hover]):not(:disabled) { border-color: var(--muted); background: var(--hover); }
  .btn:focus-visible, .btn:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; }
  .btn:disabled, .btn:global([data-sheet~=disabled]) { color: var(--disabled); cursor: default; }
  .btn.primary { color: var(--bg); background: var(--accent); border-color: var(--accent); }
  .btn.primary:hover:not(:disabled), .btn.primary:global([data-sheet~=hover]):not(:disabled) { background: var(--accent); opacity: .9; }
  .btn.warn { color: var(--warn); }
  .sugg { display: flex; flex-wrap: wrap; gap: 6px; margin-top: 8px; font-size: 11px; color: var(--muted); }
  .sugg button { padding: 0 7px; font: 11px/18px var(--font-text); color: var(--chip-fg); background: var(--chip-bg); border: var(--border) solid var(--chip-line); border-radius: var(--radius-pill); cursor: pointer; }
  .sugg button:hover, .sugg button:global([data-sheet~=hover]) { border-color: var(--muted); color: var(--ink); }
  .answer { margin-top: 12px; padding: 10px 12px; background: var(--bg); border: var(--border) solid var(--line); border-radius: var(--radius-lg); }
  .answer h3 { margin: 0 0 8px; font-size: 13px; font-weight: var(--weight-strong); }
  .answer h3 code { color: var(--accent); }
  .answer dl { padding: 0; grid-template-columns: 150px minmax(0, 1fr); }
  .answer dd { white-space: normal; }
  .yes { color: var(--live); }
  .no { color: var(--warn); }
  [data-refused] { margin: 8px 0 0; font-size: 11px; color: var(--warn); }
  [data-loading] { color: transparent; background: var(--skeleton); border-radius: var(--radius-sm); }
  [data-empty] { color: var(--muted); }
  @media (prefers-reduced-motion: reduce) { .tr, .team, .btn, .field { transition: none; } }
  @media (max-width: 860px) { .me { grid-template-columns: 1fr; } }
  @media (max-width: 720px) { .th { display: none; } .tr { grid-template-columns: 80px minmax(0, 1fr); } .tr > span:nth-child(n+3) { display: none; } }
</style>
