# Real Node Corpus Plan

This directory records the first 10 real Node app/library test cases for the
TS -> Go full parity gate.

The gate must eventually fetch or vendor pinned package source, compile it
through tsgodown, build generated Go, execute both Node and Go probes, and
compare observable behavior.

## Gate shape

- Full gate command: `pnpm run gate:node-corpus-parity`
- Node source health-check command: `pnpm run gate:node-corpus-parity:node`
- Manifest path: `test-corpus/node-real/manifest.json`
- Corpus npm workspace path: `test-corpus/node-real/package.json`
- Vendored package source path: `test-corpus/node-real/packages/<case-id>`
- Compiler entry path: `entry` in `test-corpus/node-real/manifest.json`
- Node probe: run original package code with fixed inputs.
- Go probe: run generated Go binary/project with the same inputs.
- Report: JSON diff grouped by package and capability.
- Required final mode: `allowWip=false`.

Current expected state:

- Node health-check: Green, all 10 vendored package probes execute and emit JSON.
- Full gate: Red until generated Go projects build, run, and match Node parity.
  The full gate generates fail-closed Go projects under
  `test-corpus/node-real/generated-go/<case-id>` while executable JS semantics
  are still incomplete.

## Probe paths

- `test-corpus/node-real/cases/semver/probe.mjs`
- `test-corpus/node-real/cases/minimatch/probe.mjs`
- `test-corpus/node-real/cases/qs/probe.mjs`
- `test-corpus/node-real/cases/dotenv/probe.mjs`
- `test-corpus/node-real/cases/yargs-parser/probe.mjs`
- `test-corpus/node-real/cases/js-yaml/probe.mjs`
- `test-corpus/node-real/cases/lru-cache/probe.mjs`
- `test-corpus/node-real/cases/uuid/probe.mjs`
- `test-corpus/node-real/cases/fs-extra/probe.mjs`
- `test-corpus/node-real/cases/execa/probe.mjs`

## Vendored package source paths

- `test-corpus/node-real/packages/semver`
- `test-corpus/node-real/packages/minimatch`
- `test-corpus/node-real/packages/qs`
- `test-corpus/node-real/packages/dotenv`
- `test-corpus/node-real/packages/yargs-parser`
- `test-corpus/node-real/packages/js-yaml`
- `test-corpus/node-real/packages/lru-cache`
- `test-corpus/node-real/packages/uuid`
- `test-corpus/node-real/packages/fs-extra`
- `test-corpus/node-real/packages/execa`

## 1. semver

- Package: `semver@7.8.0`
- Source: `https://github.com/npm/node-semver`
- License: ISC
- Test type: library + CLI
- Logic to probe:
  - parse valid and invalid versions
  - compare prerelease versions
  - evaluate ranges such as `^1.2.3`, `~1.2.3`, `>=1.0.0 <2`
  - sort mixed versions
  - run CLI-style input and compare stdout/exit code
- Required capabilities:
  - CJS module graph
  - classes/functions
  - RegExp
  - string/array/object semantics
  - CLI argv/stdout/exit
- Comparator:
  - exact JSON result for library probes
  - exact exit code and normalized stdout/stderr for CLI probes

## 2. minimatch

- Package: `minimatch@10.2.5`
- Source: `https://github.com/isaacs/minimatch`
- License: BlueOak-1.0.0
- Test type: library
- Logic to probe:
  - glob matching for `*.js`, `src/**/*.ts`, and dotfiles
  - brace expansion
  - extglob patterns
  - negation patterns
  - Windows-like path separator cases
- Required capabilities:
  - ESM/CJS package resolution as required by package build
  - RegExp
  - path/string semantics
  - array/object iteration
- Comparator:
  - exact JSON result matrix of pattern/path/options -> boolean

## 3. qs

- Package: `qs@6.15.1`
- Source: `https://github.com/ljharb/qs`
- License: BSD-3-Clause
- Test type: library
- Logic to probe:
  - parse nested query strings
  - parse arrays and repeated keys
  - stringify nested objects
  - encode/decode reserved characters
  - malformed input/error behavior where observable
- Required capabilities:
  - object/array recursion
  - string encoding/decoding
  - module graph
  - error behavior
- Comparator:
  - stable JSON object output
  - exact string output for stringify probes

## 4. dotenv

- Package: `dotenv@17.4.2`
- Source: `https://github.com/motdotla/dotenv`
- License: BSD-2-Clause
- Test type: library + app/config
- Logic to probe:
  - parse `.env` text with quotes, comments, escaped newlines
  - load `.env` from cwd
  - update `process.env`
  - preserve existing env unless override is requested
  - report parse/load errors
- Required capabilities:
  - fs read
  - process env
  - cwd/path handling
  - object/string semantics
- Comparator:
  - exact JSON result
  - exact `process.env` delta
  - normalized file path in errors

## 5. yargs-parser

- Package: `yargs-parser@22.0.0`
- Source: `https://github.com/yargs/yargs-parser`
- License: ISC
- Test type: library + CLI parsing
- Logic to probe:
  - short and long flags
  - aliases
  - booleans and negated booleans
  - arrays and repeated options
  - number/string coercion
- Required capabilities:
  - argv semantics
  - object/array/string operations
  - package module loading
- Comparator:
  - stable JSON parse output

## 6. js-yaml

- Package: `js-yaml@4.1.1`
- Source: `https://github.com/nodeca/js-yaml`
- License: MIT
- Test type: library + dependency graph
- Logic to probe:
  - load scalars, arrays, objects
  - dump objects to YAML
  - anchors/aliases where deterministic
  - invalid YAML exception shape
  - dependency import behavior
- Required capabilities:
  - module/dependency graph
  - parser-heavy control flow
  - exceptions
  - string/object/array semantics
- Comparator:
  - exact JSON result for load probes
  - exact or normalized string for dump probes
  - error name/message fields for invalid probes

## 7. lru-cache

- Package: `lru-cache@11.3.6`
- Source: `https://github.com/isaacs/node-lru-cache`
- License: BlueOak-1.0.0
- Test type: library
- Logic to probe:
  - set/get/has/delete behavior
  - LRU eviction order
  - iteration order
  - max size behavior
  - TTL behavior with deterministic clock
- Required capabilities:
  - classes
  - Map/Set/iterator semantics
  - object mutation
  - deterministic Date/timer subset
- Comparator:
  - exact JSON snapshots after each operation
  - deterministic clock injection for TTL probes

## 8. uuid

- Package: `uuid@13.0.2`
- Source: `https://github.com/uuidjs/uuid`
- License: MIT
- Test type: library
- Logic to probe:
  - parse/stringify UUIDs
  - validate UUIDs
  - version detection
  - deterministic v3/v5 style APIs if available
  - random UUID shape for nondeterministic APIs
- Required capabilities:
  - Buffer/Uint8Array
  - crypto random or deterministic injected random
  - string/number/array semantics
  - ESM package loading
- Comparator:
  - exact output for deterministic probes
  - regex/shape comparator for random probes

## 9. fs-extra

- Package: `fs-extra@11.3.5`
- Source: `https://github.com/jprichardson/node-fs-extra`
- License: MIT
- Test type: library + app/filesystem
- Logic to probe:
  - ensure directory
  - write/read JSON
  - copy files/directories
  - remove paths
  - promise and callback variants used by package surface
- Required capabilities:
  - fs sync/async/promise/callback subset
  - path handling
  - JSON
  - async completion order
- Comparator:
  - exact JSON result
  - filesystem tree snapshot after each probe
  - normalized temp paths

## 10. execa

- Package: `execa@9.6.1`
- Source: `https://github.com/sindresorhus/execa`
- License: MIT
- Test type: app/subprocess
- Logic to probe:
  - spawn a simple command
  - capture stdout/stderr
  - propagate env
  - handle non-zero exit
  - strip final newline behavior
- Required capabilities:
  - child_process subset
  - streams
  - process env/path
  - async/promise
  - error shape
- Comparator:
  - exact exit code
  - normalized stdout/stderr
  - observed error fields for non-zero exit

## Score report

The final report must include:

- package name/version
- source URL
- probe count
- Node status
- Go build status
- Go run status
- parity status
- capability failures grouped by `language`, `module`, `node-api`, `async`,
  `filesystem`, and `cli/process`
