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
    switch (item.code) {
      case "-32003":
        return new ForbiddenError(item.message, item.recoverable, item.metadata);
      case "-32001":
        return new UnauthenticatedError(item.message, item.recoverable, item.metadata);
      default:
        return new PlexusError(item.message, item.code, item.recoverable, item.metadata);
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
    async* answer(input, ticket, value, rationale, source) {
      const stream = this.rpc.call("tickets.answer", { input, rationale, source, ticket, value });
      yield* extractData(stream);
    }
    async* blockers(id) {
      const stream = this.rpc.call("tickets.blockers", { id });
      yield* extractData(stream);
    }
    async* clearAnswer(input, ticket, source) {
      const stream = this.rpc.call("tickets.clear_answer", { input, source, ticket });
      yield* extractData(stream);
    }
    async* closeInput(input, ticket) {
      const stream = this.rpc.call("tickets.close_input", { input, ticket });
      yield* extractData(stream);
    }
    async* components() {
      const stream = this.rpc.call("tickets.components", {});
      yield* extractData(stream);
    }
    async* counts(filter, keep, tz) {
      const stream = this.rpc.call("tickets.counts", { filter, keep, tz });
      yield* extractData(stream);
    }
    async* create(epic, title, blockedBy, body, kind, parent, path, severity) {
      const stream = this.rpc.call("tickets.create", { blocked_by: blockedBy, body, epic, kind, parent, path, severity, title });
      yield* extractData(stream);
    }
    async* extents() {
      const stream = this.rpc.call("tickets.extents", {});
      yield* extractData(stream);
    }
    async* facets(base, filter, tz) {
      const stream = this.rpc.call("tickets.facets", { base, filter, tz });
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
    async* groups() {
      const stream = this.rpc.call("tickets.groups", {});
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
    async* inputs(epic, filter, state, ticket, tz) {
      const stream = this.rpc.call("tickets.inputs", { epic, filter, state, ticket, tz });
      yield* extractData(stream);
    }
    async* label(add, id, ids, remove) {
      const stream = this.rpc.call("tickets.label", { add, id, ids, remove });
      yield* extractData(stream);
    }
    async* linkCommits(commits, id, ids) {
      const stream = this.rpc.call("tickets.link_commits", { commits, id, ids });
      yield* extractData(stream);
    }
    async* list(after, at, epic, filter, keep, limit, parent, status, summary, tz) {
      const stream = this.rpc.call("tickets.list", { after, at, epic, filter, keep, limit, parent, status, summary, tz });
      yield* extractData(stream);
    }
    async* member(filter, ids, tz) {
      const stream = this.rpc.call("tickets.member", { filter, ids, tz });
      yield* extractData(stream);
    }
    async* readySet(after, epic, filter, limit, summary, tz) {
      const stream = this.rpc.call("tickets.ready_set", { after, epic, filter, limit, summary, tz });
      yield* extractData(stream);
    }
    async* reopenInput(input, ticket) {
      const stream = this.rpc.call("tickets.reopen_input", { input, ticket });
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
    async* tree(filter, after, epic, keep, limit, matchesOnly, parent, tz) {
      const stream = this.rpc.call("tickets.tree", { after, epic, filter, keep, limit, matches_only: matchesOnly, parent, tz });
      yield* extractData(stream);
    }
    async* unlinkCommits(commits, id) {
      const stream = this.rpc.call("tickets.unlink_commits", { commits, id });
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
    async* waves(epics, kinds) {
      const stream = this.rpc.call("tickets.waves", { epics, kinds });
      yield* extractData(stream);
    }
    async* wavesTo(id, kinds) {
      const stream = this.rpc.call("tickets.waves_to", { id, kinds });
      yield* extractData(stream);
    }
    async* wavesUnder(id, kinds) {
      const stream = this.rpc.call("tickets.waves_under", { id, kinds });
      yield* extractData(stream);
    }
  }
  function createTicketsClient(rpc) {
    return new TicketsClientImpl(rpc);
  }

  // bridge.ts
  var NAMESPACE = "tickets.";
  function metadata(method) {
    return { provenance: ["menger", "bridge", method], plexusHash: "", timestamp: Math.floor(Date.now() / 1000) };
  }
  function bridgeRpc(call) {
    return {
      async* call(method, params) {
        const name = method.startsWith(NAMESPACE) ? method.slice(NAMESPACE.length) : method;
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
  mengerTyped = {
    tickets: (call) => createTicketsClient(bridgeRpc(call)),
    typed: (content) => collectOne(one(content))
  };
})();
