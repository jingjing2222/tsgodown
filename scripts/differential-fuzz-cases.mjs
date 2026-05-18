const cases = [...ecmascriptCases(), ...nodeApiCases(), ...holdoutCases()].map(
  (testCase, index) => ({
    id: `${testCase.group}-${String(index + 1).padStart(3, "0")}`,
    ...testCase,
  }),
);

export function differentialFuzzCases() {
  return cases;
}

function ecmascriptCases() {
  return Array.from({ length: 60 }, (_, index) => {
    const value = index + 1;
    return {
      group: "ecmascript",
      capability: pick(
        [
          "scope-closure",
          "destructuring-spread",
          "class-prototype",
          "coercion-equality",
          "try-finally",
          "promise-order",
        ],
        index,
      ),
      source: printResult(`
        const log = [];
        const base = { value: ${value}, nested: { flag: ${value % 2 === 0} } };
        const clone = { ...base, extra: [${value}, ${value + 1}] };
        class Box {
          constructor(input) { this.input = input; }
          get doubled() { return this.input * 2; }
        }
        function closure(seed) {
          let state = seed;
          return (delta) => { state += delta; return state; };
        }
        const next = closure(${value});
        try {
          log.push(["try", next(1)]);
        } finally {
          log.push(["finally", new Box(${value}).doubled]);
        }
        await Promise.resolve().then(() => log.push(["microtask", clone.extra.at(-1)]));
        return {
          value: clone.value,
          flag: clone.nested.flag,
          eq: ${value} == "${value}",
          strict: ${value} === "${value}",
          log
        };
      `),
    };
  });
}

function nodeApiCases() {
  return Array.from({ length: 60 }, (_, index) => {
    const value = index + 1;
    return {
      group: "node-api",
      capability: pick(
        ["path-url-buffer-crypto", "events", "querystring", "fs-temp"],
        index,
      ),
      source: printResult(`
        const path = (await import("node:path")).default;
        const { URL } = await import("node:url");
        const { Buffer } = await import("node:buffer");
        const crypto = (await import("node:crypto")).default;
        const { EventEmitter } = await import("node:events");
        const querystring = (await import("node:querystring")).default;
        const fs = (await import("node:fs")).default;
        const os = (await import("node:os")).default;
        const url = new URL("/items/${value}?q=a+b", "https://example.test/base/");
        const emitter = new EventEmitter();
        const events = [];
        emitter.on("data", (payload) => events.push(payload));
        emitter.emit("data", { value: ${value} });
        const dir = fs.mkdtempSync(path.join(os.tmpdir(), "tsgodown-fuzz-"));
        const file = path.join(dir, "value.json");
        fs.writeFileSync(file, JSON.stringify({ value: ${value} }));
        const read = JSON.parse(fs.readFileSync(file, "utf8"));
        fs.rmSync(dir, { recursive: true, force: true });
        return {
          joined: path.posix.join("a", "b", "..", "c"),
          url: { pathname: url.pathname, q: url.searchParams.get("q") },
          buffer: Buffer.from("value-${value}").toString("base64"),
          hash: crypto.createHash("sha256").update("value-${value}").digest("hex").slice(0, 12),
          query: querystring.parse("a=1&a=2&b=${value}"),
          events,
          read
        };
      `),
    };
  });
}

function holdoutCases() {
  return Array.from({ length: 30 }, (_, index) => {
    const value = index + 1;
    return {
      group: "holdout",
      capability: pick(
        ["module-cycle-shape", "async-cli-shape", "object-library-shape"],
        index,
      ),
      source: printResult(`
        const registry = new Map();
        function define(name, factory) {
          registry.set(name, { factory, exports: {}, initialized: false });
        }
        function requireLocal(name) {
          const record = registry.get(name);
          if (!record.initialized) {
            record.initialized = true;
            record.factory(record.exports, requireLocal);
          }
          return record.exports;
        }
        define("a", (exports, require) => {
          exports.name = "a-${value}";
          exports.peer = () => require("b").name;
        });
        define("b", (exports, require) => {
          exports.name = "b-${value}";
          exports.peer = () => require("a").name;
        });
        const a = requireLocal("a");
        const b = requireLocal("b");
        return {
          argvShape: process.argv.slice(0, 1).length,
          cwdType: typeof process.cwd(),
          cycle: [a.name, a.peer(), b.peer()],
          objectKeys: Object.keys({ z: 1, a: 2, [String(${value})]: 3 })
        };
      `),
    };
  });
}

function printResult(bodySource) {
  return `
    const value = await (async () => {
      ${bodySource}
    })();
    console.log(JSON.stringify(value));
  `;
}

function pick(values, index) {
  return values[index % values.length];
}
