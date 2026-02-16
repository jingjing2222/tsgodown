package main

import (
	"fmt"
	"net/http"
	"os"
	"strings"
	"time"
)

// IR diagnostics carried from rust analyzer (SSoT):
// Generated Go may be scaffold-only until these diagnostics are resolved.
// [warn] UNSUPPORTED_DYNAMIC_PATH: unsupported dynamic path in fastify.get(...). Use string literal path (e.g. '/users/:id') for IR extraction.
//   at src/server.ts:9:2
// Action: fix diagnostics in source and regenerate. Emitter does not own policy decisions.

func registerRoutes(mux *http.ServeMux) {
	mux.HandleFunc("GET /health", route0)
}

func resolveListenAddr() string {
	if addr := strings.TrimSpace(os.Getenv("TSGODOWN_ADDR")); addr != "" {
		return addr
	}
	if port := strings.TrimSpace(os.Getenv("PORT")); port != "" {
		if strings.Contains(port, ":") {
			return port
		}
		return ":" + port
	}
	return ":18081"
}

func main() {
	mux := http.NewServeMux()
	registerRoutes(mux)
	addr := resolveListenAddr()
	fmt.Println("tsgodown scaffold listening on", addr)
	server := &http.Server{
		Addr:              addr,
		Handler:           mux,
		ReadHeaderTimeout: 5 * time.Second,
	}
	if err := server.ListenAndServe(); err != nil && err != http.ErrServerClosed {
		fmt.Println("server exited:", err)
	}
}

func route0(w http.ResponseWriter, req *http.Request) {
	// Route metadata:
	//   Method: GET
	//   Path: "/health"
	//   Pattern: "GET /health"
	//   Handler: "fallback"
	//   Handler params: none
	//   Handler async: false
	//   Handler response mode: unknown
	// TODO(tsgodown): Implement handler "fallback" for GET /health.
	//   - Replace this scaffold with application logic.
	//   - Validate request input and map to domain arguments.
	//   - Write response status, headers, and body.

	w.Header().Set("Content-Type", "text/plain; charset=utf-8")
	w.WriteHeader(http.StatusNotImplemented)
	fmt.Fprintln(w, "TODO implement handler fallback for GET /health")
}
