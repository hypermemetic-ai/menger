<!-- LAYERS-31: the kinds of held child, as an activation's definition and as a client's code, with what is
     checked for each (owner, 2026-11-16: "is this defined in any ticket? if not build it into a
     compontent"). One is built — a child that is anything, `Box<dyn Activation>` — and three are sketches:
     an interface that is enforced, a child per key that is served, a child that may not be there. Every
     kind says which it is. No route: placed by its tag in a body. Nothing here calls the server; the code
     is drawn by code-card, placed by its bound mark.

     Marks: `[data-surface=held-kinds]` (`data-kind`), `[data-kind-tab=<key>]`, `[data-built=true|false]`. -->
<script module>
  export const uses = [];

  export const KINDS = [
    {
      key: "dynamic", name: "anything", built: true,
      holds: "Box<dyn Activation>",
      real: "Every name below exists in plexus v2 today, and the studio example runs this: its glaze activation is another process.",
      definition: `// A parent that holds a child it knows nothing about.
#[child]
pub fn glaze(&self) -> Box<dyn Activation> {
    Box::new(self.glaze.clone())
}

// What fills it: an activation across a channel.
let glaze = Remote::ws("ws://10.0.0.7:4470", "glaze")?;
// or another process, over its stdin and stdout:
let glaze = Remote::child(Command::new("glaze"), "glaze")?;

// Or mounted by hand, with no macro:
server.mount("studio.glaze", Box::new(glaze));`,
      client: `// No expected hash: accept whatever is served.
let (conn, drive) = Connection::new(channel, None);
tokio::spawn(drive);
let root_hash = conn.ready().await?;

// Learn what is there, then call by name, with JSON.
let places = conn.call("_tree", ()).await?;            // [{ place, hash }]
let ir     = conn.call("studio.glaze._ir", ()).await?; // its methods and types
let grams  = conn.call("studio.glaze.weigh", json!({ "recipe": "celadon" })).await?;
let heat   = conn.stream("studio.kiln.heat", json!({ "cone": 6 }))?;`,
      checks: [
        ["the place exists", "at each call", "the server: not-found"],
        ["the parameters fit", "never", "nothing: the activation gets what was sent"],
        ["what answers is what was expected", "never", "nothing was expected"],
      ],
      gap: "A caller that learned the interface is checked by nothing. A handle that reads the IR once and refuses a wrong parameter before sending would close it: conn.at(\"studio.glaze\").learn().await? — a sketch, not built.",
    },
    {
      key: "interface", name: "an interface, enforced", built: false,
      holds: "Box<dyn Glazes>",
      real: "Today `trait Glazes: plexus::Activation {}` is an empty marker: the IR records \"interface\": \"glazes\", and whatever is plugged in is mounted unchecked. `#[plexus::interface]`, `Glazes::HASH`, `fitting` and `GlazesClient` for an interface do not exist. `Error::Interface` does: a connection's root is refused with it at hello.",
      definition: `// The interface, written once. It gets a hash, as an activation does.
#[plexus::interface]
pub trait Glazes {
    fn recipes(&self) -> Vec<Recipe>;
    fn weigh(&self, recipe: String) -> Grams;
}
// generates Glazes::HASH, Glazes::IR and a typed GlazesClient

// A parent holds whatever fits it.
#[child]
pub fn supplier(&self) -> Box<dyn Glazes> {
    Box::new(self.supplier.clone())
}

// Connecting asks what is there for its hash, once.
let supplier = Remote::ws(url, "glaze")?
    .fitting::<dyn Glazes>()
    .await?; // Err(Error::Interface { expected, served })`,
      client: `// Typed calls, without knowing who answers them.
let glazes = GlazesClient::bind(&conn, "studio.bench.supplier").await?;
//                        the hash is compared here, once

let grams: Grams = glazes.weigh("celadon").await?;
let all: Vec<Recipe> = glazes.recipes().await?;`,
      checks: [
        ["a method exists, its parameters fit", "compiling", "rustc, tsc"],
        ["what fills the interface serves it", "once, when connected", "comparing two hashes"],
        ["the place serves the interface the client was made for", "once, at bind", "comparing two hashes"],
      ],
      gap: "Open: whether an activation that serves more than the interface fits it (a hash of the whole IR says no; a hash of the interface's methods alone says yes).",
    },
    {
      key: "keyed", name: "one per key", built: false,
      holds: "Option<&Shelf>, by a key",
      real: "The macro reads this form today and writes it to the IR (a `key` and a `list`). It refuses to serve it: \"a child per key is described, not served yet\". The client's `shelf(\"top\")` is not generated.",
      definition: `// One child for each key. Which keys exist is asked while running.
#[child(list = "shelves")]
pub fn shelf(&self, name: String) -> Option<&Shelf> {
    self.shelves.get(&name)
}

// The method the attribute names: the keys there are.
pub fn shelves(&self) -> Vec<String> {
    self.shelves.keys().cloned().collect()
}`,
      client: `let names = kiln.shelves().await?;   // ["top", "middle"]

// The key is a segment of the place: kiln.shelf.top
let top = kiln.shelf("top");          // no round trip
let load = top.load().await?;         // not-found here, if there is no such key

// The same, by name:
conn.call("kiln.shelf.top.load", ()).await?;`,
      checks: [
        ["a method of the child exists, its parameters fit", "compiling", "rustc, tsc"],
        ["the key exists", "at each call", "the parent: not-found"],
        ["the child's interface", "once, at bind", "one hash for every key"],
      ],
      gap: "Open: what `_tree` lists (the pattern kiln.shelf.*, not every key), and whether a key may hold a dot.",
    },
    {
      key: "absent", name: "may not be there", built: false,
      holds: "Result<Box<dyn Glazes>, Unavailable>",
      real: "The macro refuses a child that may fail today: \"a child that may fail to be there is described, not served yet\". What exists: a child across a closed connection already fails every call `unavailable`, and `_tree` lists it with no hash.",
      definition: `// Asked for at each call, not once at the start:
// the child can come and go while the parent runs.
#[child]
pub fn glaze(&self) -> Result<Box<dyn Glazes>, Unavailable> {
    self.supplier.get()
}`,
      client: `// The same typed client, and one more failure to expect.
match glazes.weigh("celadon").await {
    Err(e) if e.code() == Some("unavailable") => {
        // not there now: ask again later
    }
    other => { let grams = other?; }
}

// _tree lists the place with no hash while it is away.`,
      checks: [
        ["a method exists, its parameters fit", "compiling", "rustc, tsc"],
        ["the child is there", "at each call", "the parent: unavailable"],
        ["its interface, each time it comes back", "when it is connected again", "comparing two hashes"],
      ],
      gap: "Open: whether a stream being read when the child leaves ends with a failure or waits for it to return.",
    },
  ];
</script>

<script>
  import CodeCard from "plexus-ui:code-card";

  let { kind = "dynamic" } = $props();
  let picked = $state(null);
  const at = $derived(KINDS.find((k) => k.key === (picked ?? kind)) ?? KINDS[0]);
</script>

<section class="held-kinds" data-surface="held-kinds" data-kind={at.key}>
  <p class="hk-what"><b>What you are looking at.</b> Four ways a parent holds a child. The first is built and runs. The other three are <b>sketches</b>: proposals for how each would be written and called, built by nothing. Each says which of its names exist today.</p>
  <div class="hk-tabs" role="tablist">
    {#each KINDS as k (k.key)}
      <button type="button" role="tab" aria-selected={k.key === at.key} data-kind-tab={k.key} data-built={k.built} onclick={() => (picked = k.key)}>
        <span class="hk-name">{k.name}</span>
        <span class="hk-state">{k.built ? "built" : "sketch"}</span>
      </button>
    {/each}
  </div>
  <div class="hk-body" role="tabpanel">
    <p class="hk-holds"><span class="hk-label">the parent holds</span> <code>{at.holds}</code> <span class="hk-flag" data-built={at.built}>{at.built ? "built, running" : "a sketch, not built"}</span></p>
    <p class="hk-real"><span class="hk-label">{at.built ? "real" : "real today"}</span> {at.real}</p>
    <div class="hk-cols">
      <div>
        <h4>The definition{at.built ? "" : " (proposed)"}</h4>
        {#key at.key}<CodeCard code={at.definition} info="rust"/>{/key}
      </div>
      <div>
        <h4>The client{at.built ? "" : " (proposed)"}</h4>
        {#key at.key}<CodeCard code={at.client} info="rust"/>{/key}
      </div>
    </div>
    <h4>What is checked</h4>
    <table>
      <thead><tr><th>the check</th><th>when</th><th>by</th></tr></thead>
      <tbody>
        {#each at.checks as c (c[0])}<tr><td>{c[0]}</td><td>{c[1]}</td><td>{c[2]}</td></tr>{/each}
      </tbody>
    </table>
    <p class="hk-gap">{at.gap}</p>
  </div>
</section>

<style>
  .held-kinds { container-type: inline-size; border: 1px solid var(--line); border-radius: 8px; background: var(--panel); color: var(--ink); font-family: var(--font-text); }
  .hk-what { margin: 0; padding: 12px 14px; border-bottom: 1px solid var(--line); color: var(--muted); line-height: 1.5; }
  .hk-what b { color: var(--ink); }
  .hk-tabs { display: flex; flex-wrap: wrap; gap: 6px; padding: 10px 14px; border-bottom: 1px solid var(--line); }
  .hk-tabs button { display: flex; gap: 8px; align-items: baseline; padding: 6px 10px; border: 1px solid var(--line); border-radius: 6px; background: var(--card); color: var(--ink); font: inherit; cursor: pointer; }
  .hk-tabs button:hover, .hk-tabs button:global([data-sheet~=hover]) { background: var(--hover); }
  .hk-tabs button[aria-selected="true"], .hk-tabs button:global([data-sheet~=selected]) { background: var(--sel); border-color: var(--accent); }
  .hk-state { color: var(--muted); font-size: 0.85em; }
  .hk-tabs button[data-built="true"] .hk-state { color: var(--accent); }
  .hk-body { padding: 12px 14px; }
  .hk-body p { margin: 0 0 10px; line-height: 1.5; }
  .hk-label { color: var(--muted); margin-right: 6px; }
  .hk-flag { margin-left: 8px; padding: 1px 8px; border: 1px solid var(--line); border-radius: 10px; color: var(--warn); font-size: 0.85em; }
  .hk-flag[data-built="true"] { color: var(--accent); }
  .hk-cols { display: grid; grid-template-columns: minmax(0, 1fr); gap: 12px; }
  @container (min-width: 900px) { .hk-cols { grid-template-columns: minmax(0, 1fr) minmax(0, 1fr); } }
  h4 { margin: 8px 0 6px; color: var(--muted); font-weight: 600; }
  table { width: 100%; border-collapse: collapse; margin-bottom: 10px; }
  th, td { text-align: left; padding: 5px 8px; border-bottom: 1px solid var(--line); vertical-align: top; }
  th { color: var(--muted); font-weight: 600; }
  .hk-gap { color: var(--muted); }
</style>
