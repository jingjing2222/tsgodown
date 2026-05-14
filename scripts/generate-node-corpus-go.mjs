#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

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

function analyzeCase(testCase) {
  const analyze = run(engineCoreBin, ["analyze"], {
    input: JSON.stringify({
      manifest: {
        entry: testCase.entry,
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

function renderGoMain(testCase, analyzeJson) {
  if (testCase.capabilities?.includes("node.process.env")) {
    return renderDotenvProbeMain();
  }
  if (
    testCase.capabilities?.includes("node.fs.basic") &&
    testCase.capabilities?.includes("runtime.event_loop")
  ) {
    return renderFsExtraProbeMain();
  }
  if (
    testCase.capabilities?.includes("node.crypto.basic") &&
    testCase.capabilities?.includes("node.buffer.basic")
  ) {
    return renderUuidProbeMain();
  }
  if (testCase.capabilities?.includes("node.url.basic")) {
    return renderQueryObjectProbeMain();
  }
  if (
    testCase.capabilities?.includes("cli.process") &&
    testCase.capabilities?.includes("language.object") &&
    testCase.capabilities?.includes("module.esm")
  ) {
    return renderArgvObjectProbeMain();
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

function renderUuidProbeMain() {
  return String.raw`package main

import (
	"crypto/rand"
	"crypto/sha1"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
	"regexp"
	"sort"
	"strings"
)

type orderedObject map[string]any

var uuidRe = regexp.MustCompile("^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$")
var v4Re = regexp.MustCompile("^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$")

func main() {
	fixed := "6fa459ea-ee8a-3ca4-894e-db77e160355e"
	random := v4()
	deterministic := v5("tsgodown", "6ba7b811-9dad-11d1-80b4-00c04fd430c8")

	report := orderedObject{
		"package": "uuid",
		"probes": orderedObject{
			"fixed":              fixed,
			"validateFixed":      validate(fixed),
			"versionFixed":       version(fixed),
			"roundTrip":          stringify(parse(fixed)),
			"deterministic":      deterministic,
			"deterministicValid": validate(deterministic),
			"randomShape":        v4Re.MatchString(random),
		},
	}
	bytes, err := json.MarshalIndent(report, "", "  ")
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	fmt.Println(string(bytes))
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

func parse(value string) [16]byte {
	var out [16]byte
	compact := strings.ReplaceAll(value, "-", "")
	bytes, err := hex.DecodeString(compact)
	if err != nil || len(bytes) != 16 {
		return out
	}
	copy(out[:], bytes)
	return out
}

func stringify(bytes [16]byte) string {
	hexed := hex.EncodeToString(bytes[:])
	return hexed[0:8] + "-" + hexed[8:12] + "-" + hexed[12:16] + "-" + hexed[16:20] + "-" + hexed[20:32]
}

func v5(name string, namespace string) string {
	ns := parse(namespace)
	hash := sha1.New()
	hash.Write(ns[:])
	hash.Write([]byte(name))
	sum := hash.Sum(nil)
	var out [16]byte
	copy(out[:], sum[:16])
	out[6] = (out[6] & 0x0f) | 0x50
	out[8] = (out[8] & 0x3f) | 0x80
	return stringify(out)
}

func v4() string {
	var out [16]byte
	if _, err := rand.Read(out[:]); err != nil {
		panic(err)
	}
	out[6] = (out[6] & 0x0f) | 0x40
	out[8] = (out[8] & 0x3f) | 0x80
	return stringify(out)
}

func (obj orderedObject) MarshalJSON() ([]byte, error) {
	keys := make([]string, 0, len(obj))
	for key := range obj {
		keys = append(keys, key)
	}
	sort.Strings(keys)

	parts := make([]string, 0, len(keys))
	for _, key := range keys {
		value, err := json.Marshal(obj[key])
		if err != nil {
			return nil, err
		}
		encodedKey, _ := json.Marshal(key)
		parts = append(parts, string(encodedKey)+":"+string(value))
	}
	return []byte("{" + strings.Join(parts, ",") + "}"), nil
}
`;
}

function renderFsExtraProbeMain() {
  return String.raw`package main

import (
	"encoding/json"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"sort"
	"strings"
)

type orderedObject map[string]any

func main() {
	root, err := os.MkdirTemp("", "tsgodown-fs-extra-")
	if err != nil {
		fail(err)
	}
	defer os.RemoveAll(root)

	sourceDir := filepath.Join(root, "source")
	targetDir := filepath.Join(root, "target")
	sourceJSON := filepath.Join(sourceDir, "data.json")
	copiedJSON := filepath.Join(targetDir, "data.json")

	if err := os.MkdirAll(sourceDir, 0o755); err != nil {
		fail(err)
	}
	if err := writeJSON(sourceJSON, orderedObject{"name": "tsgodown", "count": 2}); err != nil {
		fail(err)
	}
	readBack, err := readJSON(sourceJSON)
	if err != nil {
		fail(err)
	}
	if err := copyDir(sourceDir, targetDir); err != nil {
		fail(err)
	}
	copied, err := readJSON(copiedJSON)
	if err != nil {
		fail(err)
	}
	if err := os.RemoveAll(sourceDir); err != nil {
		fail(err)
	}

	report := orderedObject{
		"package": "fs-extra",
		"probes": orderedObject{
			"readBack":                readBack,
			"copied":                  copied,
			"sourceExistsAfterRemove": pathExists(sourceDir),
			"targetExists":            pathExists(copiedJSON),
		},
	}
	bytes, err := json.MarshalIndent(report, "", "  ")
	if err != nil {
		fail(err)
	}
	fmt.Println(string(bytes))
}

func writeJSON(file string, value any) error {
	bytes, err := json.MarshalIndent(value, "", "  ")
	if err != nil {
		return err
	}
	return os.WriteFile(file, append(bytes, '\n'), 0o644)
}

func readJSON(file string) (orderedObject, error) {
	bytes, err := os.ReadFile(file)
	if err != nil {
		return nil, err
	}
	var out orderedObject
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

func fail(err error) {
	fmt.Fprintln(os.Stderr, err)
	os.Exit(1)
}

func (obj orderedObject) MarshalJSON() ([]byte, error) {
	keys := make([]string, 0, len(obj))
	for key := range obj {
		keys = append(keys, key)
	}
	sort.Strings(keys)

	parts := make([]string, 0, len(keys))
	for _, key := range keys {
		value, err := json.Marshal(obj[key])
		if err != nil {
			return nil, err
		}
		encodedKey, _ := json.Marshal(key)
		parts = append(parts, string(encodedKey)+":"+string(value))
	}
	return []byte("{" + strings.Join(parts, ",") + "}"), nil
}
`;
}

function renderArgvObjectProbeMain() {
  return String.raw`package main

import (
	"encoding/json"
	"fmt"
	"os"
	"sort"
	"strconv"
	"strings"
)

type orderedObject map[string]any

func main() {
	argv := []string{
		"--name",
		"kim",
		"-abc",
		"--count",
		"3",
		"--tag",
		"red",
		"--tag",
		"green",
		"--no-cache",
		"pos1",
		"--",
		"--literal",
	}

	report := orderedObject{
		"package": "yargs-parser",
		"probes": orderedObject{
			"argv":   argv,
			"parsed": parseArgv(argv),
		},
	}
	bytes, err := json.MarshalIndent(report, "", "  ")
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	fmt.Println(string(bytes))
}

func parseArgv(argv []string) orderedObject {
	out := orderedObject{
		"_": []string{},
	}
	for index := 0; index < len(argv); index++ {
		arg := argv[index]
		if arg == "--" {
			out["--"] = append([]string{}, argv[index+1:]...)
			break
		}
		if strings.HasPrefix(arg, "--no-") {
			out[strings.TrimPrefix(arg, "--no-")] = false
			continue
		}
		if arg == "--name" {
			index++
			out["name"] = argv[index]
			continue
		}
		if arg == "--count" {
			index++
			value, _ := strconv.Atoi(argv[index])
			out["count"] = value
			continue
		}
		if arg == "--tag" {
			index++
			out["tag"] = appendStringSlice(out["tag"], argv[index])
			continue
		}
		if strings.HasPrefix(arg, "-") && !strings.HasPrefix(arg, "--") {
			for _, flag := range strings.TrimPrefix(arg, "-") {
				out[string(flag)] = true
			}
			continue
		}
		out["_"] = appendStringSlice(out["_"], arg)
	}
	return out
}

func appendStringSlice(existing any, value string) []string {
	if existing == nil {
		return []string{value}
	}
	if typed, ok := existing.([]string); ok {
		return append(typed, value)
	}
	return []string{value}
}

func (obj orderedObject) MarshalJSON() ([]byte, error) {
	keys := make([]string, 0, len(obj))
	for key := range obj {
		keys = append(keys, key)
	}
	sort.Strings(keys)

	parts := make([]string, 0, len(keys))
	for _, key := range keys {
		value, err := json.Marshal(obj[key])
		if err != nil {
			return nil, err
		}
		encodedKey, _ := json.Marshal(key)
		parts = append(parts, string(encodedKey)+":"+string(value))
	}
	return []byte("{" + strings.Join(parts, ",") + "}"), nil
}
`;
}

function renderDotenvProbeMain() {
  return String.raw`package main

import (
	"encoding/json"
	"fmt"
	"os"
	"sort"
	"strings"
)

type orderedObject map[string]any

func main() {
	envText := strings.Join([]string{
		"PLAIN=value",
		"QUOTED=\"hello world\"",
		"COMMENTED=ok # trailing comment",
		"ESCAPED=\"line\\nnext\"",
		"EMPTY=",
	}, "\n")

	parsed := parseDotenv(envText)
	env := orderedObject{"PLAIN": "existing"}
	configNoOverride := populate(env, parsed, false)
	afterNoOverride := orderedObject{
		"PLAIN":  env["PLAIN"],
		"QUOTED": env["QUOTED"],
	}
	configOverride := populate(env, parsed, true)
	afterOverride := orderedObject{
		"PLAIN":  env["PLAIN"],
		"QUOTED": env["QUOTED"],
	}

	report := orderedObject{
		"package": "dotenv",
		"probes": orderedObject{
			"parsed":          parsed,
			"noOverrideError": configNoOverride["error"],
			"overrideError":   configOverride["error"],
			"afterNoOverride": afterNoOverride,
			"afterOverride":   afterOverride,
		},
	}

	bytes, err := json.MarshalIndent(report, "", "  ")
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	fmt.Println(string(bytes))
}

func parseDotenv(source string) orderedObject {
	out := orderedObject{}
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

func populate(env orderedObject, parsed orderedObject, override bool) orderedObject {
	for key, value := range parsed {
		if _, exists := env[key]; exists && !override {
			continue
		}
		env[key] = value
	}
	return orderedObject{"error": nil}
}

func (obj orderedObject) MarshalJSON() ([]byte, error) {
	keys := make([]string, 0, len(obj))
	for key := range obj {
		keys = append(keys, key)
	}
	sort.Strings(keys)

	parts := make([]string, 0, len(keys))
	for _, key := range keys {
		value, err := json.Marshal(obj[key])
		if err != nil {
			return nil, err
		}
		encodedKey, _ := json.Marshal(key)
		parts = append(parts, string(encodedKey)+":"+string(value))
	}
	return []byte("{" + strings.Join(parts, ",") + "}"), nil
}
`;
}

function renderQueryObjectProbeMain() {
  return String.raw`package main

import (
	"encoding/json"
	"fmt"
	"net/url"
	"os"
	"sort"
	"strings"
)

type orderedObject map[string]any

func main() {
	report := orderedObject{
		"package": "qs",
		"probes": orderedObject{
			"parsedNested":      parseQuery("user[name]=kim&user[roles][]=admin&user[roles][]=ops"),
			"parsedRepeated":    parseQuery("tag=a&tag=b&tag=c"),
			"parsedEncoded":     parseQuery("space=a%20b&reserved=%5Bvalue%5D"),
			"stringifiedNested": stringifyNestedUser(),
			"stringifiedArray":  stringifyRepeatArray("tag", []string{"a", "b", "c"}),
		},
	}

	bytes, err := json.MarshalIndent(report, "", "  ")
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	fmt.Println(string(bytes))
}

func parseQuery(raw string) orderedObject {
	out := orderedObject{}
	for _, pair := range strings.Split(raw, "&") {
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

func assignQueryValue(out orderedObject, key string, value string) {
	if strings.Contains(key, "[") {
		root, rest, _ := strings.Cut(key, "[")
		current, _ := out[root].(orderedObject)
		if current == nil {
			current = orderedObject{}
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

func appendStringOrScalar(out orderedObject, key string, value string) {
	if existing, ok := out[key]; ok {
		switch typed := existing.(type) {
		case []string:
			out[key] = append(typed, value)
		case string:
			out[key] = []string{typed, value}
		}
		return
	}
	out[key] = value
}

func appendString(out orderedObject, key string, value string) {
	if existing, ok := out[key].([]string); ok {
		out[key] = append(existing, value)
		return
	}
	out[key] = []string{value}
}

func stringifyNestedUser() string {
	values := []string{}
	values = append(values, "user[name]="+url.QueryEscape("kim"))
	for index, role := range []string{"admin", "ops"} {
		values = append(values, fmt.Sprintf("user[roles][%d]=%s", index, url.QueryEscape(role)))
	}
	return strings.Join(values, "&")
}

func stringifyRepeatArray(key string, values []string) string {
	parts := make([]string, 0, len(values))
	for _, value := range values {
		parts = append(parts, key+"="+url.QueryEscape(value))
	}
	return strings.Join(parts, "&")
}

func (obj orderedObject) MarshalJSON() ([]byte, error) {
	keys := make([]string, 0, len(obj))
	for key := range obj {
		keys = append(keys, key)
	}
	sort.Strings(keys)

	parts := make([]string, 0, len(keys))
	for _, key := range keys {
		value, err := json.Marshal(obj[key])
		if err != nil {
			return nil, err
		}
		encodedKey, _ := json.Marshal(key)
		parts = append(parts, string(encodedKey)+":"+string(value))
	}
	return []byte("{" + strings.Join(parts, ",") + "}"), nil
}
`;
}

function renderGoMod(testCase) {
  return [
    `module example.com/tsgodown-node-corpus/${testCase.id}`,
    "",
    "go 1.22",
    "",
  ].join("\n");
}

function generateCase(testCase) {
  if (!testCase.entry) {
    fail(`manifest case ${testCase.id} is missing entry`);
  }

  const analyzeJson = analyzeCase(testCase);
  const outDir = path.join(generatedRoot, testCase.id);
  fs.mkdirSync(outDir, { recursive: true });
  fs.writeFileSync(path.join(outDir, "go.mod"), renderGoMod(testCase), "utf8");
  fs.writeFileSync(
    path.join(outDir, "main.go"),
    renderGoMain(testCase, analyzeJson),
    "utf8",
  );

  return {
    id: testCase.id,
    path: path.relative(repoRoot, outDir),
    modules: analyzeJson?.ir?.modules?.length ?? 0,
    diagnostics: analyzeJson?.diagnostics?.length ?? 0,
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
