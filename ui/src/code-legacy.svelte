<!-- EDITOR-9: the small languages painted — shell, TOML, SQL, diff,
     Haskell, Dockerfile and Lua, CodeMirror's legacy modes (@codemirror/legacy-modes) as
     stream languages; one component, one module, fetched the first time
     code-card draws a block in any of them; the painting code-paint's. -->
<script module>
  export const uses = [];
</script>

<script>
  import { StreamLanguage } from "@codemirror/language";
  import { shell } from "@codemirror/legacy-modes/mode/shell";
  import { toml } from "@codemirror/legacy-modes/mode/toml";
  import { diff } from "@codemirror/legacy-modes/mode/diff";
  import { haskell } from "@codemirror/legacy-modes/mode/haskell";
  import { dockerFile } from "@codemirror/legacy-modes/mode/dockerfile";
  import { lua } from "@codemirror/legacy-modes/mode/lua";
  import { standardSQL } from "@codemirror/legacy-modes/mode/sql";
  import Paint from "./code-paint.svelte";

  let { code = "", lang = "shell" } = $props();
  const MODES = { shell, toml, sql: standardSQL, diff, haskell, dockerfile: dockerFile, lua };
  const language = $derived(MODES[lang] ? StreamLanguage.define(MODES[lang]) : null);
</script>

<Paint {code} {language} name={lang}/>
