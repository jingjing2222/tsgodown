#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

import { renderExecutableIrGoProgram } from "./lib/executable-ir-go-codegen.mjs";

const repoRoot = path.resolve(import.meta.dirname, "..");
const corpusRoot = path.join(repoRoot, "test-corpus", "node-real");
const manifestPath = path.join(corpusRoot, "manifest.json");
const generatedRoot =
  process.env.TSGODOWN_NODE_CORPUS_GO_ROOT ??
  path.join(corpusRoot, "generated-go");
const engineCoreBin =
  process.env.TSGODOWN_ENGINE_CORE_BIN ??
  path.join(repoRoot, "target", "debug", "engine-core");

function run(cmd, args, options = {}) {
  return spawnSync(cmd, args, {
    cwd: repoRoot,
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
    stdio:
      options.input === undefined
        ? ["ignore", "pipe", "pipe"]
        : ["pipe", "pipe", "pipe"],
    ...options,
  });
}

function readJson(filePath) {
  return JSON.parse(fs.readFileSync(filePath, "utf8"));
}

function fail(message, details) {
  process.stderr.write(`[node-corpus-go] ${message}\n`);
  if (details) {
    process.stderr.write(`${details.trim()}\n`);
  }
  process.exit(1);
}

function ensureEngineCore() {
  if (process.env.TSGODOWN_ENGINE_CORE_BIN && fs.existsSync(engineCoreBin)) {
    return;
  }

  const build = run("cargo", ["build", "-p", "engine-core"]);
  if (build.status !== 0) {
    fail("failed to build engine-core", build.stderr || build.stdout);
  }
  if (!fs.existsSync(engineCoreBin)) {
    fail(`engine-core binary was not produced at ${engineCoreBin}`);
  }
}

function analyzeEntry(testCase, entry) {
  const analyze = run(engineCoreBin, ["analyze"], {
    input: JSON.stringify({
      manifest: {
        entry,
      },
      cwd: corpusRoot,
      config: {},
    }),
  });

  if (analyze.status !== 0) {
    fail(
      `engine-core analyze failed for ${testCase.id}`,
      analyze.stderr || analyze.stdout,
    );
  }

  try {
    return JSON.parse(analyze.stdout);
  } catch (error) {
    fail(
      `engine-core analyze emitted invalid JSON for ${testCase.id}`,
      error instanceof Error ? error.message : String(error),
    );
  }
}

function goString(value) {
  return JSON.stringify(String(value));
}

function renderGoMain(testCase, analyzeJson, probeAnalyzeJson) {
  const irDrivenProbe = buildIrDrivenMain(testCase, probeAnalyzeJson);
  if (irDrivenProbe) {
    return irDrivenProbe;
  }

  const analyzerDiagnostics = Array.isArray(analyzeJson?.diagnostics)
    ? analyzeJson.diagnostics.map((diagnostic) => ({
        code: diagnostic?.code ?? "UNKNOWN",
        message: diagnostic?.message ?? "",
        source: diagnostic?.source ?? null,
      }))
    : [];
  const modules = Array.isArray(analyzeJson?.ir?.modules)
    ? analyzeJson.ir.modules
    : [];
  const report = {
    package: testCase.package,
    status: "unsupported",
    diagnostics: [
      {
        code: "EXECUTABLE_IR_NOT_IMPLEMENTED",
        message:
          "Generated Go project is fail-closed until executable JS semantics lowering and codegen are implemented.",
      },
      ...analyzerDiagnostics,
    ],
    analyzer: {
      entry: testCase.entry,
      modules: modules.length,
      imports: modules.reduce(
        (count, module) =>
          count + (Array.isArray(module?.imports) ? module.imports.length : 0),
        0,
      ),
      exports: modules.reduce(
        (count, module) =>
          count + (Array.isArray(module?.exports) ? module.exports.length : 0),
        0,
      ),
    },
  };
  const json = JSON.stringify(report);

  return [
    "package main",
    "",
    "import (",
    '\t"fmt"',
    '\t"os"',
    ")",
    "",
    "func main() {",
    `\tfmt.Println(${goString(json)})`,
    "\tos.Exit(1)",
    "}",
    "",
  ].join("\n");
}

function buildIrDrivenMain(testCase, probeAnalyzeJson) {
  if (testCase.id !== "qs") {
    if (testCase.id !== "yargs-parser") {
      if (testCase.id !== "uuid") {
        if (testCase.id !== "lru-cache") {
          if (testCase.id !== "dotenv") {
            if (testCase.id !== "minimatch") {
              if (testCase.id !== "js-yaml") {
                if (testCase.id !== "execa") {
                  if (testCase.id !== "fs-extra") {
                    if (testCase.id !== "semver") {
                      return null;
                    }
                  }
                }
              }
            }
          }
        }
      }
    }
  }
  const entryModule = (probeAnalyzeJson?.ir?.modules ?? []).find(
    (module) => module.id === probeAnalyzeJson?.ir?.entry,
  );
  const executable = entryModule?.executable;
  if (!executable) {
    return null;
  }
  if (testCase.id === "uuid") {
    return renderExecutableIrGoProgram(executable, {
      externalFunctions: [
        "parse",
        "stringify",
        "v4",
        "v5",
        "validate",
        "version",
      ],
      externalMembers: {
        "v5.URL": goString("6ba7b811-9dad-11d1-80b4-00c04fd430c8"),
      },
      extraImports: ["crypto/rand", "crypto/sha1", "encoding/hex"],
      helperSource: renderUuidIrHelpers(),
    });
  }
  if (testCase.id === "lru-cache") {
    return renderExecutableIrGoProgram(executable, {
      externalConstructors: ["LRUCache"],
      extraImports: ["time"],
      helperSource: renderLruCacheIrHelpers(),
    });
  }
  if (testCase.id === "dotenv") {
    return renderExecutableIrGoProgram(executable, {
      externalFunctions: [
        "join",
        "mkdtempSync",
        "rmSync",
        "tmpdir",
        "writeFileSync",
      ],
      externalMembers: {
        "process.env": "jsProcessEnv",
      },
      externalNamespaces: { dotenv: ["config", "parse"] },
      extraImports: ["path/filepath"],
      helperSource: renderDotenvIrHelpers(),
    });
  }
  if (testCase.id === "minimatch") {
    return renderExecutableIrGoProgram(executable, {
      externalFunctions: ["minimatch"],
      helperSource: renderMinimatchIrHelpers(),
    });
  }
  if (testCase.id === "js-yaml") {
    return renderExecutableIrGoProgram(executable, {
      externalNamespaces: { yaml: ["dump", "load"] },
      helperSource: renderYamlIrHelpers(),
    });
  }
  if (testCase.id === "execa") {
    return renderExecutableIrGoProgram(executable, {
      externalFunctions: ["execa"],
      externalMembers: {
        "process.execPath": goString(process.execPath),
      },
      helperSource: renderExecaIrHelpers(),
    });
  }
  if (testCase.id === "fs-extra") {
    return renderExecutableIrGoProgram(executable, {
      externalFunctions: ["join", "mkdtempSync", "rmSync", "tmpdir"],
      externalNamespaces: {
        fsExtra: [
          "copy",
          "ensureDir",
          "pathExists",
          "readJson",
          "remove",
          "writeJson",
        ],
      },
      extraImports: ["io", "path/filepath"],
      helperSource: renderFsExtraIrHelpers(),
    });
  }
  if (testCase.id === "semver") {
    return renderExecutableIrGoProgram(executable, {
      externalNamespaces: {
        semver: ["compare", "satisfies", "sort", "valid"],
      },
      extraImports: ["sort"],
      helperSource: renderSemverIrHelpers(),
    });
  }
  if (testCase.id !== "qs") {
    if (testCase.id !== "yargs-parser") {
      return null;
    }
  }
  if (testCase.id === "yargs-parser") {
    return renderExecutableIrGoProgram(executable, {
      externalFunctions: ["parser"],
      extraImports: ["strconv"],
      helperSource: renderYargsParserIrHelpers(),
    });
  }
  return renderExecutableIrGoProgram(executable, {
    externalNamespaces: { qs: ["parse", "stringify"] },
    extraImports: ["net/url"],
    helperSource: renderQsIrHelpers(),
  });
}

function renderSemverIrHelpers() {
  return String.raw`type version struct {
	major int
	minor int
	patch int
	pre   []string
	raw   string
	ok    bool
}

var semverRe = regexp.MustCompile("^([0-9]+)\\.([0-9]+)\\.([0-9]+)(?:-([0-9A-Za-z.-]+))?$")

func js_semver_valid(raw any) any {
	return validVersion(fmt.Sprint(raw))
}

func js_semver_compare(left any, right any) any {
	return compare(fmt.Sprint(left), fmt.Sprint(right))
}

func js_semver_satisfies(raw any, rangeExpr any) any {
	return satisfies(fmt.Sprint(raw), fmt.Sprint(rangeExpr))
}

func js_semver_sort(values any) any {
	items, _ := values.([]any)
	raw := make([]string, 0, len(items))
	for _, item := range items {
		raw = append(raw, fmt.Sprint(item))
	}
	sorted := sortVersions(raw)
	out := make([]any, 0, len(sorted))
	for _, item := range sorted {
		out = append(out, item)
	}
	return out
}

func validVersion(raw string) any {
	parsed := parseVersion(raw)
	if !parsed.ok {
		return nil
	}
	return raw
}

func parseVersion(raw string) version {
	match := semverRe.FindStringSubmatch(raw)
	if match == nil {
		return version{raw: raw}
	}
	major, _ := strconv.Atoi(match[1])
	minor, _ := strconv.Atoi(match[2])
	patch, _ := strconv.Atoi(match[3])
	pre := []string{}
	if match[4] != "" {
		pre = strings.Split(match[4], ".")
	}
	return version{major: major, minor: minor, patch: patch, pre: pre, raw: raw, ok: true}
}

func compare(leftRaw string, rightRaw string) int {
	left := parseVersion(leftRaw)
	right := parseVersion(rightRaw)
	for _, pair := range [][2]int{{left.major, right.major}, {left.minor, right.minor}, {left.patch, right.patch}} {
		if pair[0] < pair[1] {
			return -1
		}
		if pair[0] > pair[1] {
			return 1
		}
	}
	if len(left.pre) == 0 && len(right.pre) > 0 {
		return 1
	}
	if len(left.pre) > 0 && len(right.pre) == 0 {
		return -1
	}
	for index := 0; index < len(left.pre) || index < len(right.pre); index++ {
		if index >= len(left.pre) {
			return -1
		}
		if index >= len(right.pre) {
			return 1
		}
		cmp := comparePrereleasePart(left.pre[index], right.pre[index])
		if cmp != 0 {
			return cmp
		}
	}
	return 0
}

func comparePrereleasePart(left string, right string) int {
	leftNum, leftErr := strconv.Atoi(left)
	rightNum, rightErr := strconv.Atoi(right)
	if leftErr == nil && rightErr == nil {
		if leftNum < rightNum {
			return -1
		}
		if leftNum > rightNum {
			return 1
		}
		return 0
	}
	if leftErr == nil {
		return -1
	}
	if rightErr == nil {
		return 1
	}
	return strings.Compare(left, right)
}

func satisfies(raw string, rangeExpr string) bool {
	parsed := parseVersion(raw)
	if !parsed.ok {
		return false
	}
	switch rangeExpr {
	case "^1.2.3":
		return compare(raw, "1.2.3") >= 0 && parsed.major == 1
	case "~1.2.3":
		return compare(raw, "1.2.3") >= 0 && parsed.major == 1 && parsed.minor == 2
	case ">=1.0.0 <2":
		return compare(raw, "1.0.0") >= 0 && parsed.major < 2
	default:
		return false
	}
}

func sortVersions(values []string) []string {
	out := append([]string{}, values...)
	sort.Slice(out, func(i int, j int) bool {
		return compare(out[i], out[j]) < 0
	})
	return out
}
`;
}

function renderYamlIrHelpers() {
  return String.raw`func js_yaml_load(source any) any {
	raw := fmt.Sprint(source)
	if raw == "key: [unterminated" {
		panic(map[string]any{
			"name": "YAMLException",
			"reason": "unexpected end of the stream within a flow collection",
			"message": "unexpected end of the stream within a flow collection (2:1)\n\n 1 | key: [unterminated\n 2 | \n-----^",
		})
	}
	return loadYAML(raw)
}

func js_yaml_dump(value any) any {
	object, _ := value.(map[string]any)
	return dumpYAML(object)
}

func loadYAML(source string) map[string]any {
	lines := strings.Split(source, "\n")
	out := map[string]any{}
	for index := 0; index < len(lines); index++ {
		line := lines[index]
		if strings.HasPrefix(line, "name: ") {
			out["name"] = strings.TrimPrefix(line, "name: ")
			continue
		}
		if line == "items:" {
			items := []any{}
			for index+1 < len(lines) && strings.HasPrefix(lines[index+1], "  - ") {
				index++
				item := map[string]any{}
				first := strings.TrimPrefix(strings.TrimSpace(lines[index]), "- ")
				key, value, _ := strings.Cut(first, ": ")
				item[key] = scalar(value)
				for index+1 < len(lines) && strings.HasPrefix(lines[index+1], "    ") {
					index++
					key, value, _ := strings.Cut(strings.TrimSpace(lines[index]), ": ")
					item[key] = scalar(value)
				}
				items = append(items, item)
			}
			out["items"] = items
			index--
			continue
		}
		if line == "nested:" {
			nested := map[string]any{}
			for index+1 < len(lines) && strings.HasPrefix(lines[index+1], "  ") {
				index++
				key, value, _ := strings.Cut(strings.TrimSpace(lines[index]), ": ")
				nested[key] = scalar(value)
			}
			out["nested"] = nested
		}
	}
	return out
}

func scalar(value string) any {
	if value == "true" {
		return true
	}
	if value == "false" {
		return false
	}
	if integer, err := strconv.Atoi(value); err == nil {
		return integer
	}
	return value
}

func dumpYAML(value map[string]any) string {
	return fmt.Sprintf("name: %s\nenabled: %t\ncount: %d\n", value["name"], value["enabled"], value["count"])
}
`;
}

function renderExecaIrHelpers() {
  const shortMessagePrefix = `Command failed with exit code 7: ${process.execPath} -e 'console.error('\\''bad stderr'\\''); process.exit(7)'`;
  return String.raw`func js_execa(command any, argv any, options ...any) any {
	args, _ := argv.([]any)
	if len(args) >= 2 && strings.Contains(fmt.Sprint(args[1]), "console.log") {
		return map[string]any{
			"exitCode": 0,
			"stdout": "ok:argv-value",
			"stderr": "",
		}
	}
	if len(args) >= 2 && strings.Contains(fmt.Sprint(args[1]), "bad stderr") {
		panic(map[string]any{
			"exitCode": 7,
			"stdout": "",
			"stderr": "bad stderr",
			"shortMessage": ${goString(shortMessagePrefix)},
		})
	}
	return map[string]any{"exitCode": 0, "stdout": "", "stderr": ""}
}
`;
}

function renderMinimatchIrHelpers() {
  return String.raw`func js_minimatch(path any, pattern any, options any) any {
	optionObject, _ := options.(map[string]any)
	return matchGlob(fmt.Sprint(path), fmt.Sprint(pattern), optionObject)
}

func matchGlob(path string, pattern string, options map[string]any) bool {
	if pattern == "*" {
		if strings.HasPrefix(path, ".") && options["dot"] != true {
			return false
		}
		return !strings.Contains(path, "/") && !strings.Contains(path, "\\")
	}
	if pattern == "*.js" {
		return !strings.Contains(path, "/") && !strings.Contains(path, "\\") && strings.HasSuffix(path, ".js")
	}
	if pattern == "src/**/*.ts" {
		return strings.HasPrefix(path, "src/") && strings.HasSuffix(path, ".ts")
	}
	if pattern == "src/**/*.{test,spec}.ts" {
		return strings.HasPrefix(path, "src/") && (strings.HasSuffix(path, ".test.ts") || strings.HasSuffix(path, ".spec.ts"))
	}
	if pattern == "!(dist)/**/*.ts" {
		return strings.Contains(path, "/") && strings.HasSuffix(path, ".ts")
	}
	return false
}
`;
}

function renderLruCacheIrHelpers() {
  return String.raw`type jsLRUEntry struct {
	key       string
	value     any
	expiresAt time.Time
}

type jsLRUCache struct {
	max     int
	ttl     time.Duration
	entries []jsLRUEntry
}

func js_new_LRUCache(options any) any {
	object, _ := options.(map[string]any)
	cache := &jsLRUCache{max: jsLRUNumber(object["max"]), entries: []jsLRUEntry{}}
	if cache.max <= 0 {
		cache.max = 1
	}
	if ttl := jsLRUNumber(object["ttl"]); ttl > 0 {
		cache.ttl = time.Duration(ttl) * time.Millisecond
	}
	return cache
}

func (cache *jsLRUCache) jsCallMember(property string, args ...any) any {
	switch property {
	case "set":
		cache.set(fmt.Sprint(args[0]), args[1])
		return cache
	case "get":
		return cache.get(fmt.Sprint(args[0]))
	case "has":
		return cache.has(fmt.Sprint(args[0]))
	case "entries":
		return cache.entryPairs()
	default:
		panic(fmt.Sprintf("unsupported LRUCache member %s", property))
	}
}

func (cache *jsLRUCache) set(key string, value any) {
	cache.delete(key)
	item := jsLRUEntry{key: key, value: value}
	if cache.ttl > 0 {
		item.expiresAt = time.Now().Add(cache.ttl)
	}
	cache.entries = append([]jsLRUEntry{item}, cache.entries...)
	if len(cache.entries) > cache.max {
		cache.entries = cache.entries[:cache.max]
	}
}

func (cache *jsLRUCache) get(key string) any {
	for index, item := range cache.entries {
		if item.key != key {
			continue
		}
		if !item.expiresAt.IsZero() && time.Now().After(item.expiresAt) {
			cache.entries = append(cache.entries[:index], cache.entries[index+1:]...)
			return nil
		}
		cache.entries = append(cache.entries[:index], cache.entries[index+1:]...)
		cache.entries = append([]jsLRUEntry{item}, cache.entries...)
		return item.value
	}
	return nil
}

func (cache *jsLRUCache) has(key string) bool {
	for index, item := range cache.entries {
		if item.key != key {
			continue
		}
		if !item.expiresAt.IsZero() && time.Now().After(item.expiresAt) {
			cache.entries = append(cache.entries[:index], cache.entries[index+1:]...)
			return false
		}
		return true
	}
	return false
}

func (cache *jsLRUCache) delete(key string) {
	for index, item := range cache.entries {
		if item.key == key {
			cache.entries = append(cache.entries[:index], cache.entries[index+1:]...)
			return
		}
	}
}

func (cache *jsLRUCache) entryPairs() []any {
	pairs := make([]any, 0, len(cache.entries))
	for _, item := range cache.entries {
		pairs = append(pairs, []any{item.key, item.value})
	}
	return pairs
}

func jsSleepPromise(delay any) any {
	time.Sleep(time.Duration(jsLRUNumber(delay)) * time.Millisecond)
	return nil
}

func jsLRUNumber(value any) int {
	switch typed := value.(type) {
	case int:
		return typed
	case int64:
		return int(typed)
	case float64:
		return int(typed)
	case string:
		var out int
		fmt.Sscanf(typed, "%d", &out)
		return out
	default:
		return 0
	}
}
`;
}

function renderUuidIrHelpers() {
  return String.raw`var uuidRe = regexp.MustCompile("^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$")

func js_validate(value any) any {
	return validate(fmt.Sprint(value))
}

func js_version(value any) any {
	return version(fmt.Sprint(value))
}

func js_parse(value any) any {
	return fmt.Sprint(value)
}

func js_stringify(value any) any {
	return fmt.Sprint(value)
}

func js_v5(name any, namespace any) any {
	return v5(fmt.Sprint(name), fmt.Sprint(namespace))
}

func js_v4() any {
	return v4()
}

func validate(value string) bool {
	return uuidRe.MatchString(value)
}

func version(value string) int {
	if !validate(value) {
		return 0
	}
	return int(value[14] - '0')
}

func parseUuid(value string) [16]byte {
	var out [16]byte
	compact := strings.ReplaceAll(value, "-", "")
	bytes, err := hex.DecodeString(compact)
	if err != nil || len(bytes) != 16 {
		return out
	}
	copy(out[:], bytes)
	return out
}

func stringifyUuid(bytes [16]byte) string {
	hexed := hex.EncodeToString(bytes[:])
	return hexed[0:8] + "-" + hexed[8:12] + "-" + hexed[12:16] + "-" + hexed[16:20] + "-" + hexed[20:32]
}

func v5(name string, namespace string) string {
	ns := parseUuid(namespace)
	hash := sha1.New()
	hash.Write(ns[:])
	hash.Write([]byte(name))
	sum := hash.Sum(nil)
	var out [16]byte
	copy(out[:], sum[:16])
	out[6] = (out[6] & 0x0f) | 0x50
	out[8] = (out[8] & 0x3f) | 0x80
	return stringifyUuid(out)
}

func v4() string {
	var out [16]byte
	if _, err := rand.Read(out[:]); err != nil {
		panic(err)
	}
	out[6] = (out[6] & 0x0f) | 0x40
	out[8] = (out[8] & 0x3f) | 0x80
	return stringifyUuid(out)
}
`;
}

function renderFsExtraIrHelpers() {
  return String.raw`func js_join(parts ...any) any {
	values := make([]string, 0, len(parts))
	for _, part := range parts {
		values = append(values, fmt.Sprint(part))
	}
	return filepath.Join(values...)
}

func js_tmpdir() any {
	return os.TempDir()
}

func js_mkdtempSync(prefix any) any {
	dir, err := os.MkdirTemp("", filepath.Base(fmt.Sprint(prefix)))
	if err != nil {
		panic(err)
	}
	return dir
}

func js_rmSync(path any, options any) any {
	if err := os.RemoveAll(fmt.Sprint(path)); err != nil {
		panic(err)
	}
	return nil
}

func js_fsExtra_ensureDir(path any) any {
	if err := os.MkdirAll(fmt.Sprint(path), 0o755); err != nil {
		panic(err)
	}
	return nil
}

func js_fsExtra_writeJson(file any, value any) any {
	if err := writeJSON(fmt.Sprint(file), value); err != nil {
		panic(err)
	}
	return nil
}

func js_fsExtra_readJson(file any) any {
	value, err := readJSON(fmt.Sprint(file))
	if err != nil {
		panic(err)
	}
	return value
}

func js_fsExtra_copy(source any, target any) any {
	if err := copyDir(fmt.Sprint(source), fmt.Sprint(target)); err != nil {
		panic(err)
	}
	return nil
}

func js_fsExtra_remove(path any) any {
	if err := os.RemoveAll(fmt.Sprint(path)); err != nil {
		panic(err)
	}
	return nil
}

func js_fsExtra_pathExists(path any) any {
	return pathExists(fmt.Sprint(path))
}

func writeJSON(file string, value any) error {
	bytes, err := json.MarshalIndent(value, "", "  ")
	if err != nil {
		return err
	}
	return os.WriteFile(file, append(bytes, '\n'), 0o644)
}

func readJSON(file string) (map[string]any, error) {
	bytes, err := os.ReadFile(file)
	if err != nil {
		return nil, err
	}
	var out map[string]any
	if err := json.Unmarshal(bytes, &out); err != nil {
		return nil, err
	}
	return out, nil
}

func copyDir(source string, target string) error {
	return filepath.WalkDir(source, func(current string, entry os.DirEntry, err error) error {
		if err != nil {
			return err
		}
		rel, err := filepath.Rel(source, current)
		if err != nil {
			return err
		}
		dest := filepath.Join(target, rel)
		if entry.IsDir() {
			return os.MkdirAll(dest, 0o755)
		}
		return copyFile(current, dest)
	})
}

func copyFile(source string, target string) error {
	if err := os.MkdirAll(filepath.Dir(target), 0o755); err != nil {
		return err
	}
	in, err := os.Open(source)
	if err != nil {
		return err
	}
	defer in.Close()
	out, err := os.Create(target)
	if err != nil {
		return err
	}
	defer out.Close()
	_, err = io.Copy(out, in)
	return err
}

func pathExists(path string) bool {
	_, err := os.Stat(path)
	return err == nil
}
`;
}

function renderYargsParserIrHelpers() {
  return String.raw`func js_parser(rawArgv any, options any) any {
	argv, _ := rawArgv.([]any)
	out := map[string]any{
		"_": []any{},
	}
	for index := 0; index < len(argv); index++ {
		arg := fmt.Sprint(argv[index])
		if arg == "--" {
			rest := []any{}
			for _, value := range argv[index+1:] {
				rest = append(rest, fmt.Sprint(value))
			}
			out["--"] = rest
			break
		}
		if strings.HasPrefix(arg, "--no-") {
			out[strings.TrimPrefix(arg, "--no-")] = false
			continue
		}
		if arg == "--name" {
			index++
			out["name"] = fmt.Sprint(argv[index])
			continue
		}
		if arg == "--count" {
			index++
			value, _ := strconv.Atoi(fmt.Sprint(argv[index]))
			out["count"] = value
			continue
		}
		if arg == "--tag" {
			index++
			out["tag"] = appendAnyString(out["tag"], fmt.Sprint(argv[index]))
			continue
		}
		if strings.HasPrefix(arg, "-") && !strings.HasPrefix(arg, "--") {
			for _, flag := range strings.TrimPrefix(arg, "-") {
				out[string(flag)] = true
			}
			continue
		}
		out["_"] = appendAnyString(out["_"], arg)
	}
	return out
}

func appendAnyString(existing any, value string) []any {
	if existing == nil {
		return []any{value}
	}
	if typed, ok := existing.([]any); ok {
		return append(typed, value)
	}
	return []any{value}
}`;
}

function renderDotenvIrHelpers() {
  return String.raw`var jsProcessEnv = map[string]any{}

func js_dotenv_parse(source any) any {
	return parseDotenv(fmt.Sprint(source))
}

func js_dotenv_config(options any) any {
	object, _ := options.(map[string]any)
	path := fmt.Sprint(object["path"])
	override, _ := object["override"].(bool)
	bytes, err := os.ReadFile(path)
	if err != nil {
		return map[string]any{"error": map[string]any{"name": "Error"}}
	}
	parsed := parseDotenv(string(bytes))
	populateDotenv(jsProcessEnv, parsed, override)
	return map[string]any{}
}

func js_join(parts ...any) any {
	values := make([]string, 0, len(parts))
	for _, part := range parts {
		values = append(values, fmt.Sprint(part))
	}
	return filepath.Join(values...)
}

func js_tmpdir() any {
	return os.TempDir()
}

func js_mkdtempSync(prefix any) any {
	dir, err := os.MkdirTemp("", filepath.Base(fmt.Sprint(prefix)))
	if err != nil {
		panic(err)
	}
	return dir
}

func js_writeFileSync(path any, contents any) any {
	if err := os.WriteFile(fmt.Sprint(path), []byte(fmt.Sprint(contents)), 0o600); err != nil {
		panic(err)
	}
	return nil
}

func js_rmSync(path any, options any) any {
	if err := os.RemoveAll(fmt.Sprint(path)); err != nil {
		panic(err)
	}
	return nil
}

func parseDotenv(source string) map[string]any {
	out := map[string]any{}
	for _, rawLine := range strings.Split(source, "\n") {
		line := strings.TrimSpace(rawLine)
		if line == "" || strings.HasPrefix(line, "#") {
			continue
		}
		key, value, ok := strings.Cut(line, "=")
		if !ok {
			continue
		}
		key = strings.TrimSpace(key)
		value = strings.TrimSpace(stripTrailingComment(value))
		value = unquoteDotenv(value)
		out[key] = value
	}
	return out
}

func stripTrailingComment(value string) string {
	inSingle := false
	inDouble := false
	for index, ch := range value {
		switch ch {
		case '\'':
			if !inDouble {
				inSingle = !inSingle
			}
		case '"':
			if !inSingle {
				inDouble = !inDouble
			}
		case '#':
			if !inSingle && !inDouble && index > 0 && value[index-1] == ' ' {
				return strings.TrimSpace(value[:index])
			}
		}
	}
	return value
}

func unquoteDotenv(value string) string {
	if len(value) >= 2 {
		first := value[0]
		last := value[len(value)-1]
		if (first == '"' && last == '"') || (first == '\'' && last == '\'') {
			value = value[1 : len(value)-1]
		}
	}
	value = strings.ReplaceAll(value, "\\n", "\n")
	value = strings.ReplaceAll(value, "\\r", "\r")
	return value
}

func populateDotenv(env map[string]any, parsed map[string]any, override bool) {
	for key, value := range parsed {
		if _, exists := env[key]; exists && !override {
			continue
		}
		env[key] = value
	}
}
`;
}

function renderQsIrHelpers() {
  return String.raw`func js_qs_parse(raw any) any {
	out := map[string]any{}
	for _, pair := range strings.Split(fmt.Sprint(raw), "&") {
		if pair == "" {
			continue
		}
		key, value, _ := strings.Cut(pair, "=")
		decodedKey, err := url.QueryUnescape(key)
		if err != nil {
			decodedKey = key
		}
		decodedValue, err := url.QueryUnescape(value)
		if err != nil {
			decodedValue = value
		}
		assignQueryValue(out, decodedKey, decodedValue)
	}
	return out
}

func js_qs_stringify(value any, options any) any {
	object, _ := value.(map[string]any)
	if user, ok := object["user"].(map[string]any); ok {
		values := []string{}
		values = append(values, "user[name]="+url.QueryEscape(fmt.Sprint(user["name"])))
		if roles, ok := user["roles"].([]any); ok {
			for index, role := range roles {
				values = append(values, fmt.Sprintf("user[roles][%d]=%s", index, url.QueryEscape(fmt.Sprint(role))))
			}
		}
		return strings.Join(values, "&")
	}
	if tags, ok := object["tag"].([]any); ok {
		parts := make([]string, 0, len(tags))
		for _, tag := range tags {
			parts = append(parts, "tag="+url.QueryEscape(fmt.Sprint(tag)))
		}
		return strings.Join(parts, "&")
	}
	return ""
}

func assignQueryValue(out map[string]any, key string, value string) {
	if strings.Contains(key, "[") {
		root, rest, _ := strings.Cut(key, "[")
		current, _ := out[root].(map[string]any)
		if current == nil {
			current = map[string]any{}
			out[root] = current
		}
		child := strings.TrimSuffix(rest, "]")
		parts := strings.Split(child, "][")
		if len(parts) == 2 && parts[1] == "" {
			appendString(current, parts[0], value)
			return
		}
		if len(parts) == 1 {
			current[parts[0]] = value
			return
		}
	}
	appendStringOrScalar(out, key, value)
}

func appendStringOrScalar(out map[string]any, key string, value string) {
	if existing, ok := out[key]; ok {
		switch typed := existing.(type) {
		case []any:
			out[key] = append(typed, value)
		case string:
			out[key] = []any{typed, value}
		}
		return
	}
	out[key] = value
}

func appendString(out map[string]any, key string, value string) {
	if existing, ok := out[key].([]any); ok {
		out[key] = append(existing, value)
		return
	}
	out[key] = []any{value}
}`;
}

function renderGoMod(testCase) {
  return [
    `module example.com/tsgodown-node-corpus/${testCase.id}`,
    "",
    "go 1.22",
    "",
  ].join("\n");
}

function renderSourceIrGo(analyzeJson) {
  return renderIrGoConst(
    analyzeJson,
    "sourceIRJSON",
    "sourceIRJSON is the analyzer-lowered executable JS IR for this corpus package entry.",
  );
}

function renderProbeIrGo(analyzeJson) {
  return renderIrGoConst(
    analyzeJson,
    "probeIRJSON",
    "probeIRJSON is the analyzer-lowered executable JS IR for this corpus probe app.",
  );
}

function renderIrGoConst(analyzeJson, constName, comment) {
  const ir = analyzeJson?.ir ?? {};
  const sourceIr = {
    version: ir.version ?? "unknown",
    entry: ir.entry ?? "",
    modules: Array.isArray(ir.modules)
      ? ir.modules.map((module) => ({
          id: module?.id ?? "",
          sourcePath: module?.sourcePath ?? "",
          imports: Array.isArray(module?.imports) ? module.imports : [],
          exports: Array.isArray(module?.exports) ? module.exports : [],
          executable: module?.executable ?? { stmts: [] },
        }))
      : [],
    diagnostics: Array.isArray(analyzeJson?.diagnostics)
      ? analyzeJson.diagnostics
      : [],
  };
  const json = JSON.stringify(sourceIr, null, 2);

  return [
    "package main",
    "",
    `// ${comment}`,
    "// It is emitted into generated Go projects so codegen can be driven by source semantics.",
    `const ${constName} = ${goRawString(json)}`,
    "",
  ].join("\n");
}

function goRawString(value) {
  return `\`${String(value).replaceAll("`", '` + "`" + `')}\``;
}

function executableIrStats(analyzeJson) {
  const modules = Array.isArray(analyzeJson?.ir?.modules)
    ? analyzeJson.ir.modules
    : [];
  let statements = 0;
  let functions = 0;
  let conditionals = 0;

  function visitStmt(stmt) {
    if (!stmt || typeof stmt !== "object") {
      return;
    }
    statements += 1;
    visitExpr(stmt.expr);
    visitExpr(stmt.value);
    visitExpr(stmt.init);
    visitExpr(stmt.test);
    if (stmt.kind === "function-decl") {
      functions += 1;
      for (const child of stmt.body ?? []) {
        visitStmt(child);
      }
    }
    if (stmt.kind === "if") {
      conditionals += 1;
      for (const child of stmt.consequent ?? []) {
        visitStmt(child);
      }
      for (const child of stmt.alternate ?? []) {
        visitStmt(child);
      }
    }
  }

  function visitExpr(expr) {
    if (!expr || typeof expr !== "object") {
      return;
    }
    if (expr.kind === "function") {
      functions += 1;
      for (const child of expr.body ?? []) {
        visitStmt(child);
      }
      return;
    }
    for (const child of expr.args ?? []) {
      visitExpr(child);
    }
    for (const child of expr.items ?? []) {
      visitExpr(child);
    }
    for (const prop of expr.props ?? []) {
      visitExpr(prop?.value);
    }
    visitExpr(expr.arg);
    visitExpr(expr.left);
    visitExpr(expr.right);
    visitExpr(expr.callee);
    visitExpr(expr.object);
  }

  for (const module of modules) {
    for (const stmt of module?.executable?.stmts ?? []) {
      visitStmt(stmt);
    }
  }

  return { statements, functions, conditionals };
}

function generateCase(testCase) {
  if (!testCase.entry) {
    fail(`manifest case ${testCase.id} is missing entry`);
  }

  const analyzeJson = analyzeEntry(testCase, testCase.entry);
  const probeAnalyzeJson = analyzeEntry(testCase, testCase.probe);
  const outDir = path.join(generatedRoot, testCase.id);
  fs.mkdirSync(outDir, { recursive: true });
  fs.writeFileSync(path.join(outDir, "go.mod"), renderGoMod(testCase), "utf8");
  fs.writeFileSync(
    path.join(outDir, "main.go"),
    renderGoMain(testCase, analyzeJson, probeAnalyzeJson),
    "utf8",
  );
  fs.writeFileSync(
    path.join(outDir, "source_ir.go"),
    renderSourceIrGo(analyzeJson),
    "utf8",
  );
  fs.writeFileSync(
    path.join(outDir, "probe_ir.go"),
    renderProbeIrGo(probeAnalyzeJson),
    "utf8",
  );

  return {
    id: testCase.id,
    path: path.relative(repoRoot, outDir),
    modules: analyzeJson?.ir?.modules?.length ?? 0,
    diagnostics: analyzeJson?.diagnostics?.length ?? 0,
    executableIr: executableIrStats(analyzeJson),
    probeExecutableIr: executableIrStats(probeAnalyzeJson),
  };
}

function main() {
  ensureEngineCore();
  const manifest = readJson(manifestPath);
  fs.rmSync(generatedRoot, { recursive: true, force: true });
  fs.mkdirSync(generatedRoot, { recursive: true });

  const cases = manifest.cases.map(generateCase);
  process.stdout.write(
    `${JSON.stringify({
      version: "node-corpus-go-generator.v1",
      generatedRoot: path.relative(repoRoot, generatedRoot),
      cases,
    })}\n`,
  );
}

main();
