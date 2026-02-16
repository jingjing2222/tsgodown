package main

import (
	"fmt"
	"net/http"
	"os"
	"strings"
	"time"
)

func registerRoutes(mux *http.ServeMux) {
	mux.HandleFunc("PATCH /api/v2/users/{id}/devices/{deviceId}", route0)
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
	//   Method: PATCH
	//   Path: "/api/v2/users/:id/devices/{deviceId}"
	//   Pattern: "PATCH /api/v2/users/{id}/devices/{deviceId}"
	//   Handler: "nested"
	//   Handler params: none
	//   Handler async: false
	//   Handler response mode: unknown
	// TODO(tsgodown): Implement handler "nested" for PATCH /api/v2/users/:id/devices/{deviceId}.
	//   - Replace this scaffold with application logic.
	//   - Validate request input and map to domain arguments.
	//   - Write response status, headers, and body.

	// Extracted path params:
	id := req.PathValue("id")
	_ = id
	deviceId := req.PathValue("deviceId")
	_ = deviceId

	w.Header().Set("Content-Type", "text/plain; charset=utf-8")
	w.WriteHeader(http.StatusNotImplemented)
	fmt.Fprintln(w, "TODO implement handler nested for PATCH /api/v2/users/:id/devices/{deviceId}")
}
