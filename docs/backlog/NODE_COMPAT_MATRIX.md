# Node Compatibility Matrix (Initial)

| Area | API/Feature | Status | Strategy |
|---|---|---|---|
| fs | readFile/writeFile | TODO | Go native os/io wrappers |
| path | join/resolve/dirname | TODO | Go path/filepath map |
| url | URL, URLSearchParams | TODO | Go net/url map |
| process | env, argv, cwd | TODO | runtime adapter |
| timers | setTimeout/setInterval | TODO | goroutine scheduler shim |
| events | EventEmitter | TODO | runtime-go emitter |
| crypto | randomUUID/hash | TODO | Go crypto/* mapping |
| buffer | Buffer basics | TODO | byte slice wrapper |
| module | ESM/CJS bridge | TODO | pipeline-level transform |

Legend: TODO / WIP / DONE / BLOCKED
