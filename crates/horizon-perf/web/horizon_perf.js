// The JavaScript side of horizon_perf.wasm (docs/perf-platform.md §7.1).
//
// The wasm does the computing; this file only moves bytes across the C ABI and frees what it is
// given. A reply is `[kind: u8][len: u32 little endian][payload]`, where kind 0 is JSON, 1 is
// bytes and 2 is text; failures are kind-0 `{"ok": false, "error": {"code", "message"}}`.
//
//   const perf = await HorizonPerf.load("/perf/horizon_perf.wasm");
//   const handle = perf.open(await file.arrayBuffer());
//   const reply = perf.query(handle, { op: "hot", filters: { limit: 25 } });
//   perf.close(handle);

const JSON_REPLY = 0;
const BYTES_REPLY = 1;
const TEXT_REPLY = 2;

export class HorizonPerfError extends Error {
  constructor(code, message) {
    super(message || code);
    this.name = "HorizonPerfError";
    this.code = code;
  }
}

export class HorizonPerf {
  constructor(exports) {
    this.x = exports;
    this.encoder = new TextEncoder();
    this.decoder = new TextDecoder();
  }

  /** Fetches and instantiates the wasm at `url`. */
  static async load(url) {
    const response = await fetch(url);
    if (!response.ok) {
      throw new HorizonPerfError("wasm_missing", `${url}: ${response.status}`);
    }
    const instantiate = (source) => WebAssembly.instantiate(source, {});
    let module;
    if (WebAssembly.instantiateStreaming) {
      try {
        return new HorizonPerf(
          (await WebAssembly.instantiateStreaming(response, {})).instance.exports,
        );
      } catch {
        // A wrong content type (a file:// page, a proxy) falls back to the bytes.
        module = await instantiate(await response.arrayBuffer());
      }
    } else {
      module = await instantiate(await response.arrayBuffer());
    }
    return new HorizonPerf(module.instance.exports);
  }

  /** Decodes a `.hzperf` (a Uint8Array or an ArrayBuffer) and returns its handle. */
  open(bytes) {
    const data = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes);
    const reply = this.#pass(data, (ptr, len) => this.x.hp_open(ptr, len));
    const value = this.#decode(reply);
    if (value?.ok === false) throw this.#failure(reply, value.error);
    return value.handle;
  }

  /** Runs a query request; returns §6.3's envelope, whose `ok` says whether the query answered. */
  query(handle, request) {
    return this.#decode(this.#request(handle, request, this.x.hp_query));
  }

  /** The §5 summary text. Throws when the capture cannot be summarized. */
  summary(handle, request = {}) {
    const reply = this.#request(handle, request, this.x.hp_summary);
    if (reply.kind === TEXT_REPLY) return this.decoder.decode(reply.payload);
    throw this.#failure(reply);
  }

  /** Compares B against A: `{text, reply}`. */
  diff(a, b, request = {}) {
    const reply = this.#request(
      [a, b],
      request,
      (ptr, len, [x, y]) => this.x.hp_diff(x, y, ptr, len),
    );
    return this.#decode(reply);
  }

  /** pprof, folded or Perfetto bytes: `{kind}` or a bare format name. */
  export(handle, request) {
    const reply = this.#request(handle, request, this.x.hp_export);
    if (reply.kind !== BYTES_REPLY) throw this.#failure(reply);
    return reply.payload;
  }

  /** Forgets a decoded capture. */
  close(handle) {
    this.x.hp_close(handle);
  }

  /**
   * Calls `run(ptr, len)` with `request` (JSON, or a string) in the module's memory, where `handle`
   * is the handle or pair of handles the export takes.
   */
  #request(handle, request, run) {
    const body = typeof request === "string" ? request : JSON.stringify(request);
    return this.#pass(this.encoder.encode(body), (ptr, len) => {
      if (Array.isArray(handle)) return run(ptr, len, handle);
      return run(handle, ptr, len);
    });
  }

  /** Copies `data` in, runs `call`, frees the input, and reads the reply. */
  #pass(data, call) {
    const ptr = this.x.hp_alloc(data.length);
    new Uint8Array(this.x.memory.buffer, ptr, data.length).set(data);
    let reply;
    try {
      reply = this.#read(call(ptr, data.length));
    } finally {
      this.x.hp_free(ptr, data.length);
    }
    return reply;
  }

  /** Reads `[kind][len][payload]` at `ptr` and frees it. */
  #read(ptr) {
    const head = new DataView(this.x.memory.buffer, ptr, 5);
    const kind = head.getUint8(0);
    const len = head.getUint32(1, true);
    const payload = new Uint8Array(this.x.memory.buffer, ptr + 5, len).slice();
    this.x.hp_free(ptr, 5 + len);
    return { kind, payload };
  }

  #decode(reply) {
    if (reply.kind !== JSON_REPLY) throw this.#failure(reply);
    return JSON.parse(this.decoder.decode(reply.payload));
  }

  #failure(reply, error) {
    let detail = error;
    if (!detail) {
      try {
        detail = JSON.parse(this.decoder.decode(reply.payload)).error;
      } catch {
        detail = { code: "internal", message: this.decoder.decode(reply.payload) };
      }
    }
    return new HorizonPerfError(detail?.code ?? "internal", detail?.message);
  }
}
