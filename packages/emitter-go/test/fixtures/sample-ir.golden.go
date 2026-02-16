package main

import (
	"fmt"
	"net/http"
	"os"
	"strings"
	"time"
)

func registerRoutes(mux *http.ServeMux) {
	mux.HandleFunc("GET /health", route0)
	mux.HandleFunc("POST /users/{id}", route1)
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
	//   Handler: "health"
	//   Handler params: request:req, response:reply
	//   Handler async: false
	//   Handler response mode: response-object
	//   Middleware: ["auth"]
	// TODO(tsgodown): Implement handler "health" for GET /health.
	//   - Replace this scaffold with application logic.
	//   - Validate request input and map to domain arguments.
	//   - Write response status, headers, and body.

	w.Header().Set("Content-Type", "text/plain; charset=utf-8")
	w.WriteHeader(http.StatusNotImplemented)
	fmt.Fprintln(w, "TODO implement handler health for GET /health")
}

func route1(w http.ResponseWriter, req *http.Request) {
	// Route metadata:
	//   Method: POST
	//   Path: "/users/:id"
	//   Pattern: "POST /users/{id}"
	//   Handler: "createUser"
	//   Handler params: request:req
	//   Handler async: true
	//   Handler response mode: return
	// TODO(tsgodown): Implement handler "createUser" for POST /users/:id.
	//   - Replace this scaffold with application logic.
	//   - Validate request input and map to domain arguments.
	//   - Write response status, headers, and body.

	// Extracted path params:
	id := req.PathValue("id")
	_ = id

	w.Header().Set("Content-Type", "text/plain; charset=utf-8")
	w.WriteHeader(http.StatusNotImplemented)
	fmt.Fprintln(w, "TODO implement handler createUser for POST /users/:id")
}
