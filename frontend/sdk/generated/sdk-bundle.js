(() => {
  var __defProp = Object.defineProperty;
  var __getOwnPropNames = Object.getOwnPropertyNames;
  var __getOwnPropDesc = Object.getOwnPropertyDescriptor;
  var __hasOwnProp = Object.prototype.hasOwnProperty;
  function __accessProp(key) {
    return this[key];
  }
  var __toCommonJS = (from) => {
    var entry = (__moduleCache ??= new WeakMap).get(from), desc;
    if (entry)
      return entry;
    entry = __defProp({}, "__esModule", { value: true });
    if (from && typeof from === "object" || typeof from === "function") {
      for (var key of __getOwnPropNames(from))
        if (!__hasOwnProp.call(entry, key))
          __defProp(entry, key, {
            get: __accessProp.bind(from, key),
            enumerable: !(desc = __getOwnPropDesc(from, key)) || desc.enumerable
          });
    }
    __moduleCache.set(from, entry);
    return entry;
  };
  var __moduleCache;

  // bridge.ts
  var exports_bridge = {};

  // generated/types.ts
  class PlexusError extends Error {
    code;
    recoverable;
    metadata;
    constructor(message, code, recoverable = false, metadata) {
      super(message);
      this.name = "PlexusError";
      this.code = code;
      this.recoverable = recoverable;
      this.metadata = metadata;
    }
  }

  class ForbiddenError extends PlexusError {
    missingScope;
    constructor(message, recoverable = false, metadata) {
      super(message, "-32003", recoverable, metadata);
      this.name = "ForbiddenError";
      const m = /missing required scope '([^']+)'/.exec(message);
      this.missingScope = m ? m[1] : undefined;
    }
  }

  class UnauthenticatedError extends PlexusError {
    constructor(message, recoverable = false, metadata) {
      super(message, "-32001", recoverable, metadata);
      this.name = "UnauthenticatedError";
    }
  }
  function errorFromStreamItem(item) {
    if (item.error_kind === "forbidden" && typeof item.grant === "string") {
      return new Forbidden(item.message, {
        grant: item.grant,
        scope: item.scope ?? "",
        roles: item.roles ?? [],
        heldOn: item.held_on ?? []
      }, item.recoverable, item.metadata);
    }
    switch (item.code) {
      case "-32003":
        return new ForbiddenError(item.message, item.recoverable, item.metadata);
      case "-32001":
        return new UnauthenticatedError(item.message, item.recoverable, item.metadata);
      default:
        return new PlexusError(item.message, item.code, item.recoverable, item.metadata);
    }
  }

  class Forbidden extends ForbiddenError {
    grant;
    scope;
    roles;
    heldOn;
    constructor(message, refusal, recoverable = false, metadata) {
      super(message, recoverable, metadata);
      this.name = "Forbidden";
      this.grant = refusal.grant;
      this.scope = refusal.scope;
      this.roles = refusal.roles;
      this.heldOn = refusal.heldOn;
    }
  }

  // generated/rpc.ts
  function toCamelCase(str) {
    return str.replace(/_([a-z])/g, (_, letter) => letter.toUpperCase());
  }
  function transformKeys(obj) {
    if (obj === null || obj === undefined)
      return obj;
    if (typeof obj !== "object")
      return obj;
    if (Array.isArray(obj))
      return obj.map(transformKeys);
    const result = {};
    for (const [key, value] of Object.entries(obj)) {
      const camelKey = toCamelCase(key);
      result[camelKey] = transformKeys(value);
    }
    return result;
  }
  async function* extractData(stream) {
    for await (const item of stream) {
      switch (item.type) {
        case "data":
          yield transformKeys(item.content);
          break;
        case "error":
          throw errorFromStreamItem(item);
        case "progress":
          break;
        case "done":
          return;
      }
    }
  }
  async function collectOne(stream) {
    for await (const item of stream) {
      switch (item.type) {
        case "data":
          return transformKeys(item.content);
        case "error":
          throw errorFromStreamItem(item);
        case "progress":
          break;
        case "done":
          break;
      }
    }
    throw new Error("No data received from method call");
  }

  // generated/tickets/client.ts
  class TicketsClientImpl {
    rpc;
    constructor(rpc) {
      this.rpc = rpc;
    }
    async* amend(id, body, title) {
      const stream = this.rpc.call("tickets.amend", { body, id, title });
      yield* extractData(stream);
    }
    async* archive(dryRun, filter, ids, max, message, tz) {
      const stream = this.rpc.call("tickets.archive", { dry_run: dryRun, filter, ids, max, message, tz });
      yield* extractData(stream);
    }
    async* awaitReply(id, input, since, timeoutMinutes) {
      const stream = this.rpc.call("tickets.await_reply", { id, input, since, timeout_minutes: timeoutMinutes });
      yield* extractData(stream);
    }
    async* create(epic, title, blockedBy, body, kind, org, parent, path, severity) {
      const stream = this.rpc.call("tickets.create", { blocked_by: blockedBy, body, epic, kind, org, parent, path, severity, title });
      yield* extractData(stream);
    }
    async* delete(children, filter, ids, max, message, tz, unblock) {
      const stream = this.rpc.call("tickets.delete", { children, filter, ids, max, message, tz, unblock });
      yield* extractData(stream);
    }
    async* find(query, after, filter, limit, tz) {
      const stream = this.rpc.call("tickets.find", { after, filter, limit, query, tz });
      yield* extractData(stream);
    }
    async* get(id) {
      const stream = this.rpc.call("tickets.get", { id });
      yield* extractData(stream);
    }
    async* health() {
      const stream = this.rpc.call("tickets.health", {});
      yield* extractData(stream);
    }
    async* history(id) {
      const stream = this.rpc.call("tickets.history", { id });
      yield* extractData(stream);
    }
    async* label(add, id, ids, remove) {
      const stream = this.rpc.call("tickets.label", { add, id, ids, remove });
      yield* extractData(stream);
    }
    async* list(after, at, epic, filter, keep, limit, parent, status, summary, tz) {
      const stream = this.rpc.call("tickets.list", { after, at, epic, filter, keep, limit, parent, status, summary, tz });
      yield* extractData(stream);
    }
    async* queue(id, reason, urgency, input, source) {
      const stream = this.rpc.call("tickets.queue", { id, input, reason, source, urgency });
      yield* extractData(stream);
    }
    async* queued(snoozed) {
      const stream = this.rpc.call("tickets.queued", { snoozed });
      yield* extractData(stream);
    }
    async* requestChanges(body, id, source) {
      const stream = this.rpc.call("tickets.request_changes", { body, id, source });
      yield* extractData(stream);
    }
    async* reshortcode(ids, to, message, newScope, parent, withChildren) {
      const stream = this.rpc.call("tickets.reshortcode", { ids, message, new_scope: newScope, parent, to, with_children: withChildren });
      yield* extractData(stream);
    }
    async* restore(dryRun, filter, ids, max, message, tz, withChildren) {
      const stream = this.rpc.call("tickets.restore", { dry_run: dryRun, filter, ids, max, message, tz, with_children: withChildren });
      yield* extractData(stream);
    }
    async schema() {
      const stream = this.rpc.call("tickets.schema", {});
      return collectOne(stream);
    }
    async* search(query, filter, limit, offset, tz) {
      const stream = this.rpc.call("tickets.search", { filter, limit, offset, query, tz });
      yield* extractData(stream);
    }
    async* setKind(id, kind, message, source, status) {
      const stream = this.rpc.call("tickets.set_kind", { id, kind, message, source, status });
      yield* extractData(stream);
    }
    async* setParent(id, parent) {
      const stream = this.rpc.call("tickets.set_parent", { id, parent });
      yield* extractData(stream);
    }
    async* setStatus(id, status, message, restoreChildren, source) {
      const stream = this.rpc.call("tickets.set_status", { id, message, restore_children: restoreChildren, source, status });
      yield* extractData(stream);
    }
    async* snooze(id, minutes, source) {
      const stream = this.rpc.call("tickets.snooze", { id, minutes, source });
      yield* extractData(stream);
    }
    async* undelete(ids) {
      const stream = this.rpc.call("tickets.undelete", { ids });
      yield* extractData(stream);
    }
    async* unqueue(id, source) {
      const stream = this.rpc.call("tickets.unqueue", { id, source });
      yield* extractData(stream);
    }
    async* update(id, block, body, kind, message, parent, severity, source, status, title, unblock) {
      const stream = this.rpc.call("tickets.update", { block, body, id, kind, message, parent, severity, source, status, title, unblock });
      yield* extractData(stream);
    }
    async* updateMany(block, filter, ids, kind, label, max, message, parent, severity, source, status, tz, unblock, unlabel) {
      const stream = this.rpc.call("tickets.update_many", { block, filter, ids, kind, label, max, message, parent, severity, source, status, tz, unblock, unlabel });
      yield* extractData(stream);
    }
    async* watch(epic, filter, membership, tz) {
      const stream = this.rpc.call("tickets.watch", { epic, filter, membership, tz });
      yield* extractData(stream);
    }
  }
  function createTicketsClient(rpc) {
    return new TicketsClientImpl(rpc);
  }

  // generated/tickets/inputs/client.ts
  class TicketsInputsClientImpl {
    rpc;
    constructor(rpc) {
      this.rpc = rpc;
    }
    async* answer(input, ticket, value, rationale) {
      const stream = this.rpc.call("tickets.inputs.answer", { input, rationale, ticket, value });
      yield* extractData(stream);
    }
    async* clear(input, ticket) {
      const stream = this.rpc.call("tickets.inputs.clear", { input, ticket });
      yield* extractData(stream);
    }
    async* close(input, ticket) {
      const stream = this.rpc.call("tickets.inputs.close", { input, ticket });
      yield* extractData(stream);
    }
    async* list(epic, filter, state, ticket, tz) {
      const stream = this.rpc.call("tickets.inputs.list", { epic, filter, state, ticket, tz });
      yield* extractData(stream);
    }
    async* reopen(input, ticket) {
      const stream = this.rpc.call("tickets.inputs.reopen", { input, ticket });
      yield* extractData(stream);
    }
    async schema() {
      const stream = this.rpc.call("tickets.inputs.schema", {});
      return collectOne(stream);
    }
  }
  function createTicketsInputsClient(rpc) {
    return new TicketsInputsClientImpl(rpc);
  }

  // generated/tickets/work/client.ts
  class TicketsWorkClientImpl {
    rpc;
    constructor(rpc) {
      this.rpc = rpc;
    }
    async* blockers(id) {
      const stream = this.rpc.call("tickets.work.blockers", { id });
      yield* extractData(stream);
    }
    async* ready(after, epic, filter, limit, summary, tz) {
      const stream = this.rpc.call("tickets.work.ready", { after, epic, filter, limit, summary, tz });
      yield* extractData(stream);
    }
    async schema() {
      const stream = this.rpc.call("tickets.work.schema", {});
      return collectOne(stream);
    }
    async* waves(epics, kinds) {
      const stream = this.rpc.call("tickets.work.waves", { epics, kinds });
      yield* extractData(stream);
    }
    async* wavesTo(id, kinds) {
      const stream = this.rpc.call("tickets.work.waves_to", { id, kinds });
      yield* extractData(stream);
    }
    async* wavesUnder(id, kinds) {
      const stream = this.rpc.call("tickets.work.waves_under", { id, kinds });
      yield* extractData(stream);
    }
  }
  function createTicketsWorkClient(rpc) {
    return new TicketsWorkClientImpl(rpc);
  }

  // generated/tickets/nav/client.ts
  class TicketsNavClientImpl {
    rpc;
    constructor(rpc) {
      this.rpc = rpc;
    }
    async* counts(filter, keep, tz) {
      const stream = this.rpc.call("tickets.nav.counts", { filter, keep, tz });
      yield* extractData(stream);
    }
    async* extents() {
      const stream = this.rpc.call("tickets.nav.extents", {});
      yield* extractData(stream);
    }
    async* facets(base, filter, tz) {
      const stream = this.rpc.call("tickets.nav.facets", { base, filter, tz });
      yield* extractData(stream);
    }
    async* groups() {
      const stream = this.rpc.call("tickets.nav.groups", {});
      yield* extractData(stream);
    }
    async* member(filter, ids, tz) {
      const stream = this.rpc.call("tickets.nav.member", { filter, ids, tz });
      yield* extractData(stream);
    }
    async schema() {
      const stream = this.rpc.call("tickets.nav.schema", {});
      return collectOne(stream);
    }
    async* tree(filter, after, epic, keep, limit, matchesOnly, parent, tz) {
      const stream = this.rpc.call("tickets.nav.tree", { after, epic, filter, keep, limit, matches_only: matchesOnly, parent, tz });
      yield* extractData(stream);
    }
  }
  function createTicketsNavClient(rpc) {
    return new TicketsNavClientImpl(rpc);
  }

  // generated/tickets/components/client.ts
  class TicketsComponentsClientImpl {
    rpc;
    constructor(rpc) {
      this.rpc = rpc;
    }
    async* list() {
      const stream = this.rpc.call("tickets.components.list", {});
      yield* extractData(stream);
    }
    async schema() {
      const stream = this.rpc.call("tickets.components.schema", {});
      return collectOne(stream);
    }
  }
  function createTicketsComponentsClient(rpc) {
    return new TicketsComponentsClientImpl(rpc);
  }

  // bridge.ts
  var NAMESPACE = "tickets.";
  var NAV = "tickets.nav.";
  var HOST_NAMES = {
    "inputs.list": "inputs",
    "work.waves": "waves",
    "work.blockers": "blockers",
    "components.list": "components"
  };
  function metadata(method) {
    return { provenance: ["menger", "bridge", method], plexusHash: "", timestamp: Math.floor(Date.now() / 1000) };
  }
  function bridgeRpc(call) {
    return {
      async* call(method, params) {
        const bare = method.startsWith(NAMESPACE) ? method.slice(NAMESPACE.length) : method;
        const name = HOST_NAMES[bare] ?? (method.startsWith(NAV) ? method.slice(NAV.length) : bare);
        let items;
        try {
          items = await call(name, params ?? null);
        } catch (e) {
          const message = e instanceof Error ? e.message : String(e);
          yield { type: "error", metadata: metadata(method), message, recoverable: false };
          return;
        }
        for (const content of items) {
          yield { type: "data", metadata: metadata(method), contentType: method, content };
        }
        yield { type: "done", metadata: metadata(method) };
      }
    };
  }
  async function* one(content) {
    yield { type: "data", metadata: metadata("typed"), contentType: "typed", content };
  }
  function frameTickets(rpc) {
    const tickets = createTicketsClient(rpc);
    const child = createTicketsInputsClient(rpc);
    const inputs = Object.assign(child.list.bind(child), {
      list: child.list.bind(child),
      answer: child.answer.bind(child),
      clear: child.clear.bind(child),
      close: child.close.bind(child),
      reopen: child.reopen.bind(child),
      schema: child.schema.bind(child)
    });
    Object.defineProperty(tickets, "inputs", { value: inputs });
    const work = createTicketsWorkClient(rpc);
    Object.assign(tickets, { work, waves: work.waves.bind(work), blockers: work.blockers.bind(work) });
    Object.defineProperty(tickets, "nav", { value: createTicketsNavClient(rpc) });
    const registry = createTicketsComponentsClient(rpc);
    const components = Object.assign(registry.list.bind(registry), {
      list: registry.list.bind(registry),
      schema: registry.schema.bind(registry)
    });
    Object.defineProperty(tickets, "components", { value: components });
    return tickets;
  }
  mengerTyped = {
    tickets: (call) => frameTickets(bridgeRpc(call)),
    typed: (content) => collectOne(one(content))
  };
})();
