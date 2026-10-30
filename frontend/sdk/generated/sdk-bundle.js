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
  async function collectOne(stream) {
    for await (const item of stream) {
      switch (item.type) {
        case "data":
          return transformKeys(item.content);
        case "error":
          throw remoteCallError(undefined, item);
        case "progress":
          break;
        case "done":
          break;
      }
    }
    throw new Error("No data received from method call");
  }
  var REMOTE_CALL_ERROR = Symbol.for("plexus.RemoteCallError");
  function stampRemote(target, method, error) {
    const e = error;
    const fields = {
      method,
      error,
      error_kind: e.error_kind,
      grant: e.grant,
      scope: e.scope,
      roles: e.roles,
      held_on: e.held_on
    };
    for (const [k, v] of Object.entries(fields)) {
      if (!Object.prototype.hasOwnProperty.call(target, k)) {
        Object.defineProperty(target, k, { value: v, enumerable: true });
      }
    }
    Object.defineProperty(target, REMOTE_CALL_ERROR, { value: true });
  }
  function remoteCallError(method, item) {
    const thrown = errorFromStreamItem(item);
    const { type: _type, metadata: _metadata, ...rest } = item;
    stampRemote(thrown, method, { ...rest, code: item.code ?? "" });
    return thrown;
  }

  class MethodStream {
    method;
    #raw;
    #items;
    constructor(method, stream) {
      this.method = method;
      this.#raw = stream;
    }
    #take(how) {
      const raw = this.#raw;
      if (!raw) {
        throw new TypeError(`${this.method}: ${how} on a stream already being read — a MethodStream is read once`);
      }
      this.#raw = undefined;
      return raw;
    }
    #iterator() {
      this.#items ??= decodeItems(this.method, this.#take("for await"));
      return this.#items;
    }
    [Symbol.asyncIterator]() {
      return this;
    }
    next() {
      return this.#iterator().next();
    }
    return(_value) {
      if (this.#items)
        return this.#items.return(undefined);
      const raw = this.#raw;
      this.#raw = undefined;
      return (raw ? raw.return(undefined) : Promise.resolve()).then(() => ({ value: undefined, done: true }));
    }
    throw(e) {
      return this.#iterator().throw(e);
    }
    events() {
      return tagEvents(this.method, decodeItems(this.method, this.#take(".events()")));
    }
    withProgress() {
      return decodeWithProgress(this.method, this.#take(".withProgress()"));
    }
  }
  async function* decodeItems(method, stream) {
    for await (const item of stream) {
      switch (item.type) {
        case "data":
          yield transformKeys(item.content);
          break;
        case "error":
          throw remoteCallError(method, item);
        case "done":
          return;
        default:
          break;
      }
    }
    return;
  }
  async function* tagEvents(method, items) {
    for await (const event of items)
      yield { method, event };
    return;
  }
  async function* decodeWithProgress(method, stream) {
    for await (const item of stream) {
      switch (item.type) {
        case "data":
          yield { item: transformKeys(item.content) };
          break;
        case "progress":
          yield { progress: item.percentage === undefined ? { message: item.message } : { message: item.message, percentage: item.percentage } };
          break;
        case "error":
          throw remoteCallError(method, item);
        case "done":
          return;
        default:
          break;
      }
    }
    return;
  }

  // generated/tickets/commits/client.ts
  class TicketsCommitsClientImpl {
    rpc;
    constructor(rpc) {
      this.rpc = rpc;
    }
    link(commits, id, ids) {
      const stream = this.rpc.call("tickets.commits.link", { commits, id, ids });
      return new MethodStream("tickets.commits.link", stream);
    }
    async schema(method) {
      const stream = this.rpc.call("tickets.commits.schema", { method });
      return collectOne(stream);
    }
    unlink(commits, id) {
      const stream = this.rpc.call("tickets.commits.unlink", { commits, id });
      return new MethodStream("tickets.commits.unlink", stream);
    }
  }
  function createTicketsCommitsClient(rpc) {
    return new TicketsCommitsClientImpl(rpc);
  }

  // generated/tickets/components/client.ts
  class TicketsComponentsClientImpl {
    rpc;
    constructor(rpc) {
      this.rpc = rpc;
    }
    list() {
      const stream = this.rpc.call("tickets.components.list", {});
      return new MethodStream("tickets.components.list", stream);
    }
    async schema(method) {
      const stream = this.rpc.call("tickets.components.schema", { method });
      return collectOne(stream);
    }
  }
  function createTicketsComponentsClient(rpc) {
    return new TicketsComponentsClientImpl(rpc);
  }

  // generated/tickets/filters/client.ts
  class TicketsFiltersClientImpl {
    rpc;
    constructor(rpc) {
      this.rpc = rpc;
    }
    check(filter) {
      const stream = this.rpc.call("tickets.filters.check", { filter });
      return new MethodStream("tickets.filters.check", stream);
    }
    delete(name) {
      const stream = this.rpc.call("tickets.filters.delete", { name });
      return new MethodStream("tickets.filters.delete", stream);
    }
    get(name) {
      const stream = this.rpc.call("tickets.filters.get", { name });
      return new MethodStream("tickets.filters.get", stream);
    }
    list() {
      const stream = this.rpc.call("tickets.filters.list", {});
      return new MethodStream("tickets.filters.list", stream);
    }
    rename(name, to) {
      const stream = this.rpc.call("tickets.filters.rename", { name, to });
      return new MethodStream("tickets.filters.rename", stream);
    }
    save(filter, name) {
      const stream = this.rpc.call("tickets.filters.save", { filter, name });
      return new MethodStream("tickets.filters.save", stream);
    }
    async schema(method) {
      const stream = this.rpc.call("tickets.filters.schema", { method });
      return collectOne(stream);
    }
  }
  function createTicketsFiltersClient(rpc) {
    return new TicketsFiltersClientImpl(rpc);
  }

  // generated/tickets/inputs/client.ts
  class TicketsInputsClientImpl {
    rpc;
    constructor(rpc) {
      this.rpc = rpc;
    }
    answer(input, ticket, value, rationale) {
      const stream = this.rpc.call("tickets.inputs.answer", { input, rationale, ticket, value });
      return new MethodStream("tickets.inputs.answer", stream);
    }
    ask(question, ticket) {
      const stream = this.rpc.call("tickets.inputs.ask", { question, ticket });
      return new MethodStream("tickets.inputs.ask", stream);
    }
    clear(input, ticket) {
      const stream = this.rpc.call("tickets.inputs.clear", { input, ticket });
      return new MethodStream("tickets.inputs.clear", stream);
    }
    close(input, ticket) {
      const stream = this.rpc.call("tickets.inputs.close", { input, ticket });
      return new MethodStream("tickets.inputs.close", stream);
    }
    list(epic, filter, state, ticket, tz) {
      const stream = this.rpc.call("tickets.inputs.list", { epic, filter, state, ticket, tz });
      return new MethodStream("tickets.inputs.list", stream);
    }
    reopen(input, ticket) {
      const stream = this.rpc.call("tickets.inputs.reopen", { input, ticket });
      return new MethodStream("tickets.inputs.reopen", stream);
    }
    async schema(method) {
      const stream = this.rpc.call("tickets.inputs.schema", { method });
      return collectOne(stream);
    }
  }
  function createTicketsInputsClient(rpc) {
    return new TicketsInputsClientImpl(rpc);
  }

  // generated/tickets/nav/client.ts
  class TicketsNavClientImpl {
    rpc;
    constructor(rpc) {
      this.rpc = rpc;
    }
    counts(filter, keep, tz) {
      const stream = this.rpc.call("tickets.nav.counts", { filter, keep, tz });
      return new MethodStream("tickets.nav.counts", stream);
    }
    extents() {
      const stream = this.rpc.call("tickets.nav.extents", {});
      return new MethodStream("tickets.nav.extents", stream);
    }
    facets(base, filter, tz) {
      const stream = this.rpc.call("tickets.nav.facets", { base, filter, tz });
      return new MethodStream("tickets.nav.facets", stream);
    }
    groups() {
      const stream = this.rpc.call("tickets.nav.groups", {});
      return new MethodStream("tickets.nav.groups", stream);
    }
    member(filter, ids, tz) {
      const stream = this.rpc.call("tickets.nav.member", { filter, ids, tz });
      return new MethodStream("tickets.nav.member", stream);
    }
    async schema(method) {
      const stream = this.rpc.call("tickets.nav.schema", { method });
      return collectOne(stream);
    }
    tree(filter, after, epic, keep, limit, matchesOnly, parent, tz) {
      const stream = this.rpc.call("tickets.nav.tree", { after, epic, filter, keep, limit, matches_only: matchesOnly, parent, tz });
      return new MethodStream("tickets.nav.tree", stream);
    }
  }
  function createTicketsNavClient(rpc) {
    return new TicketsNavClientImpl(rpc);
  }

  // generated/tickets/revisions/client.ts
  class TicketsRevisionsClientImpl {
    rpc;
    constructor(rpc) {
      this.rpc = rpc;
    }
    diff(id, from, text, to) {
      const stream = this.rpc.call("tickets.revisions.diff", { from, id, text, to });
      return new MethodStream("tickets.revisions.diff", stream);
    }
    get(id, rev) {
      const stream = this.rpc.call("tickets.revisions.get", { id, rev });
      return new MethodStream("tickets.revisions.get", stream);
    }
    list(id, before, limit) {
      const stream = this.rpc.call("tickets.revisions.list", { before, id, limit });
      return new MethodStream("tickets.revisions.list", stream);
    }
    restore(id, rev, fields, message, source) {
      const stream = this.rpc.call("tickets.revisions.restore", { fields, id, message, rev, source });
      return new MethodStream("tickets.revisions.restore", stream);
    }
    async schema(method) {
      const stream = this.rpc.call("tickets.revisions.schema", { method });
      return collectOne(stream);
    }
  }
  function createTicketsRevisionsClient(rpc) {
    return new TicketsRevisionsClientImpl(rpc);
  }

  // generated/tickets/subtasks/client.ts
  class TicketsSubtasksClientImpl {
    rpc;
    constructor(rpc) {
      this.rpc = rpc;
    }
    create(parent, title, blockedBy, body, kind) {
      const stream = this.rpc.call("tickets.subtasks.create", { blocked_by: blockedBy, body, kind, parent, title });
      return new MethodStream("tickets.subtasks.create", stream);
    }
    list(parent, after, limit) {
      const stream = this.rpc.call("tickets.subtasks.list", { after, limit, parent });
      return new MethodStream("tickets.subtasks.list", stream);
    }
    async schema(method) {
      const stream = this.rpc.call("tickets.subtasks.schema", { method });
      return collectOne(stream);
    }
  }
  function createTicketsSubtasksClient(rpc) {
    return new TicketsSubtasksClientImpl(rpc);
  }

  // generated/tickets/work/client.ts
  class TicketsWorkClientImpl {
    rpc;
    constructor(rpc) {
      this.rpc = rpc;
    }
    blockers(id) {
      const stream = this.rpc.call("tickets.work.blockers", { id });
      return new MethodStream("tickets.work.blockers", stream);
    }
    ready(after, epic, filter, limit, summary, tz) {
      const stream = this.rpc.call("tickets.work.ready", { after, epic, filter, limit, summary, tz });
      return new MethodStream("tickets.work.ready", stream);
    }
    async schema(method) {
      const stream = this.rpc.call("tickets.work.schema", { method });
      return collectOne(stream);
    }
    waves(epics, kinds) {
      const stream = this.rpc.call("tickets.work.waves", { epics, kinds });
      return new MethodStream("tickets.work.waves", stream);
    }
    wavesTo(id, kinds) {
      const stream = this.rpc.call("tickets.work.waves_to", { id, kinds });
      return new MethodStream("tickets.work.waves_to", stream);
    }
    wavesUnder(id, kinds) {
      const stream = this.rpc.call("tickets.work.waves_under", { id, kinds });
      return new MethodStream("tickets.work.waves_under", stream);
    }
  }
  function createTicketsWorkClient(rpc) {
    return new TicketsWorkClientImpl(rpc);
  }

  // generated/tickets/client.ts
  class TicketsClientImpl {
    rpc;
    commits;
    components;
    filters;
    inputs;
    nav;
    revisions;
    subtasks;
    work;
    constructor(rpc) {
      this.rpc = rpc;
      this.commits = createTicketsCommitsClient(rpc);
      this.components = createTicketsComponentsClient(rpc);
      this.filters = createTicketsFiltersClient(rpc);
      this.inputs = createTicketsInputsClient(rpc);
      this.nav = createTicketsNavClient(rpc);
      this.revisions = createTicketsRevisionsClient(rpc);
      this.subtasks = createTicketsSubtasksClient(rpc);
      this.work = createTicketsWorkClient(rpc);
    }
    amend(id, body, title) {
      const stream = this.rpc.call("tickets.amend", { body, id, title });
      return new MethodStream("tickets.amend", stream);
    }
    archive(dryRun, filter, ids, max, message, tz) {
      const stream = this.rpc.call("tickets.archive", { dry_run: dryRun, filter, ids, max, message, tz });
      return new MethodStream("tickets.archive", stream);
    }
    assign(add, id, ids, message, remove, source) {
      const stream = this.rpc.call("tickets.assign", { add, id, ids, message, remove, source });
      return new MethodStream("tickets.assign", stream);
    }
    awaitReply(id, input, since, timeoutMinutes) {
      const stream = this.rpc.call("tickets.await_reply", { id, input, since, timeout_minutes: timeoutMinutes });
      return new MethodStream("tickets.await_reply", stream);
    }
    create(epic, title, blockedBy, body, chat, kind, org, parent, path, severity) {
      const stream = this.rpc.call("tickets.create", { blocked_by: blockedBy, body, chat, epic, kind, org, parent, path, severity, title });
      return new MethodStream("tickets.create", stream);
    }
    delete(children, filter, ids, max, message, tz, unblock) {
      const stream = this.rpc.call("tickets.delete", { children, filter, ids, max, message, tz, unblock });
      return new MethodStream("tickets.delete", stream);
    }
    find(query, after, filter, limit, tz) {
      const stream = this.rpc.call("tickets.find", { after, filter, limit, query, tz });
      return new MethodStream("tickets.find", stream);
    }
    freeze(id) {
      const stream = this.rpc.call("tickets.freeze", { id });
      return new MethodStream("tickets.freeze", stream);
    }
    get(id) {
      const stream = this.rpc.call("tickets.get", { id });
      return new MethodStream("tickets.get", stream);
    }
    health() {
      const stream = this.rpc.call("tickets.health", {});
      return new MethodStream("tickets.health", stream);
    }
    history(id) {
      const stream = this.rpc.call("tickets.history", { id });
      return new MethodStream("tickets.history", stream);
    }
    label(add, id, ids, remove) {
      const stream = this.rpc.call("tickets.label", { add, id, ids, remove });
      return new MethodStream("tickets.label", stream);
    }
    list(after, at, epic, filter, keep, limit, parent, status, summary, tz) {
      const stream = this.rpc.call("tickets.list", { after, at, epic, filter, keep, limit, parent, status, summary, tz });
      return new MethodStream("tickets.list", stream);
    }
    queue(id, reason, urgency, blocking, input, message, mockup, source) {
      const stream = this.rpc.call("tickets.queue", { blocking, id, input, message, mockup, reason, source, urgency });
      return new MethodStream("tickets.queue", stream);
    }
    queued(snoozed) {
      const stream = this.rpc.call("tickets.queued", { snoozed });
      return new MethodStream("tickets.queued", stream);
    }
    rename(id, to, dryRun, message) {
      const stream = this.rpc.call("tickets.rename", { dry_run: dryRun, id, message, to });
      return new MethodStream("tickets.rename", stream);
    }
    requestChanges(body, id, source) {
      const stream = this.rpc.call("tickets.request_changes", { body, id, source });
      return new MethodStream("tickets.request_changes", stream);
    }
    reshortcode(ids, to, dryRun, message, newScope, parent, withChildren) {
      const stream = this.rpc.call("tickets.reshortcode", { dry_run: dryRun, ids, message, new_scope: newScope, parent, to, with_children: withChildren });
      return new MethodStream("tickets.reshortcode", stream);
    }
    restore(dryRun, filter, ids, max, message, tz, withChildren) {
      const stream = this.rpc.call("tickets.restore", { dry_run: dryRun, filter, ids, max, message, tz, with_children: withChildren });
      return new MethodStream("tickets.restore", stream);
    }
    async schema(method) {
      const stream = this.rpc.call("tickets.schema", { method });
      return collectOne(stream);
    }
    search(query, filter, limit, offset, tz) {
      const stream = this.rpc.call("tickets.search", { filter, limit, offset, query, tz });
      return new MethodStream("tickets.search", stream);
    }
    setKind(id, kind, message, source, status) {
      const stream = this.rpc.call("tickets.set_kind", { id, kind, message, source, status });
      return new MethodStream("tickets.set_kind", stream);
    }
    setParent(id, parent) {
      const stream = this.rpc.call("tickets.set_parent", { id, parent });
      return new MethodStream("tickets.set_parent", stream);
    }
    setStatus(id, status, message, restoreChildren, source) {
      const stream = this.rpc.call("tickets.set_status", { id, message, restore_children: restoreChildren, source, status });
      return new MethodStream("tickets.set_status", stream);
    }
    snooze(id, minutes, source) {
      const stream = this.rpc.call("tickets.snooze", { id, minutes, source });
      return new MethodStream("tickets.snooze", stream);
    }
    undelete(ids) {
      const stream = this.rpc.call("tickets.undelete", { ids });
      return new MethodStream("tickets.undelete", stream);
    }
    unfreeze(id) {
      const stream = this.rpc.call("tickets.unfreeze", { id });
      return new MethodStream("tickets.unfreeze", stream);
    }
    unqueue(id, source) {
      const stream = this.rpc.call("tickets.unqueue", { id, source });
      return new MethodStream("tickets.unqueue", stream);
    }
    update(id, block, body, hidden, kind, message, parent, severity, source, status, title, unblock) {
      const stream = this.rpc.call("tickets.update", { block, body, hidden, id, kind, message, parent, severity, source, status, title, unblock });
      return new MethodStream("tickets.update", stream);
    }
    updateMany(assign, block, filter, ids, kind, label, max, message, parent, severity, source, status, tz, unassign, unblock, unlabel) {
      const stream = this.rpc.call("tickets.update_many", { assign, block, filter, ids, kind, label, max, message, parent, severity, source, status, tz, unassign, unblock, unlabel });
      return new MethodStream("tickets.update_many", stream);
    }
    watch(epic, filter, membership, revisions, tz) {
      const stream = this.rpc.call("tickets.watch", { epic, filter, membership, revisions, tz });
      return new MethodStream("tickets.watch", stream);
    }
  }
  function createTicketsClient(rpc) {
    return new TicketsClientImpl(rpc);
  }

  // bridge.ts
  var NAMESPACE = "tickets.";
  function metadata(method) {
    return { provenance: ["menger", "bridge", method], plexus_hash: "", timestamp: Math.floor(Date.now() / 1000) };
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
          yield { type: "data", metadata: metadata(method), content_type: method, content };
        }
        yield { type: "done", metadata: metadata(method) };
      }
    };
  }
  async function* one(content) {
    yield { type: "data", metadata: metadata("typed"), content_type: "typed", content };
  }
  mengerTyped = {
    tickets: (call) => createTicketsClient(bridgeRpc(call)),
    typed: (content) => collectOne(one(content))
  };
})();
