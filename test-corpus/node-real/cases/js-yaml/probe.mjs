import yaml from "../../packages/js-yaml/dist/js-yaml.mjs";

const source = [
  "name: tsgodown",
  "items:",
  "  - id: 1",
  "    ok: true",
  "  - id: 2",
  "    ok: false",
  "nested:",
  "  value: hello",
].join("\n");

let invalid;
try {
  yaml.load("key: [unterminated");
} catch (error) {
  invalid = {
    name: error.name,
    reason: error.reason ?? null,
    messagePrefix: String(error.message).split("\n")[0],
  };
}

const report = {
  package: "js-yaml",
  probes: {
    loaded: yaml.load(source),
    dumped: yaml.dump({ name: "tsgodown", enabled: true, count: 2 }),
    invalid,
  },
};

console.log(JSON.stringify(report, null, 2));
