package main

import (
	"fmt"
	"net/http"
	"os"
	"strings"
	"time"
)

func registerRoutes(mux *http.ServeMux) {
	mux.HandleFunc("GET /things/{type}/{req}/{w}/{pathParamType}", route0)
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
	//   Path: "/things/:type/:req/:w/:pathParamType"
	//   Pattern: "GET /things/{type}/{req}/{w}/{pathParamType}"
	//   Handler: "keywordParams"
	//   Handler params: none
	//   Handler async: false
	//   Handler response mode: unknown
	// TODO(tsgodown): Implement handler "keywordParams" for GET /things/:type/:req/:w/:pathParamType.
	//   - Replace this scaffold with application logic.
	//   - Validate request input and map to domain arguments.
	//   - Write response status, headers, and body.

	// Extracted path params:
	pathParamType := req.PathValue("type")
	_ = pathParamType
	pathParamReq := req.PathValue("req")
	_ = pathParamReq
	pathParamW := req.PathValue("w")
	_ = pathParamW
	pathParamType2 := req.PathValue("pathParamType")
	_ = pathParamType2

	w.Header().Set("Content-Type", "text/plain; charset=utf-8")
	w.WriteHeader(http.StatusNotImplemented)
	fmt.Fprintln(w, "TODO implement handler keywordParams for GET /things/:type/:req/:w/:pathParamType")
}
