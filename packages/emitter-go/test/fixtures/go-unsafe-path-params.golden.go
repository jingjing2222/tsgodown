package main

import (
	"encoding/json"
	"fmt"
	"net/http"
	"os"
	"sort"
	"strings"
	"time"
)

type runtimeRouter struct {
	methodMux *http.ServeMux
	pathMux *http.ServeMux
	methodsByPathPattern map[string]map[string]struct{}
	pathPatternRegistered map[string]struct{}
}

func newRuntimeRouter() *runtimeRouter {
	return &runtimeRouter{
		methodMux:             http.NewServeMux(),
		pathMux:               http.NewServeMux(),
		methodsByPathPattern:  map[string]map[string]struct{}{},
		pathPatternRegistered: map[string]struct{}{},
	}
}

func (r *runtimeRouter) handle(method string, pathPattern string, handler http.HandlerFunc) {
	r.methodMux.HandleFunc(method+" "+pathPattern, handler)
	if _, ok := r.pathPatternRegistered[pathPattern]; !ok {
		r.pathMux.HandleFunc(pathPattern, func(http.ResponseWriter, *http.Request) {})
		r.pathPatternRegistered[pathPattern] = struct{}{}
	}
	methodSet, ok := r.methodsByPathPattern[pathPattern]
	if !ok {
		methodSet = map[string]struct{}{}
		r.methodsByPathPattern[pathPattern] = methodSet
	}
	methodSet[method] = struct{}{}
	if method == http.MethodGet {
		methodSet[http.MethodHead] = struct{}{}
	}
}

func (r *runtimeRouter) ServeHTTP(w http.ResponseWriter, req *http.Request) {
	handler, methodPattern := r.methodMux.Handler(req)
	if methodPattern != "" {
		handler.ServeHTTP(w, req)
		return
	}

	_, pathPattern := r.pathMux.Handler(req)
	if pathPattern != "" {
		allow := make([]string, 0, len(r.methodsByPathPattern[pathPattern]))
		for method := range r.methodsByPathPattern[pathPattern] {
			allow = append(allow, method)
		}
		sort.Strings(allow)
		w.Header().Set("Allow", strings.Join(allow, ", "))
		http.Error(w, http.StatusText(http.StatusMethodNotAllowed), http.StatusMethodNotAllowed)
		return
	}

	http.NotFound(w, req)
}

func registerRoutes(router *runtimeRouter) {
	router.handle("GET", "/things/{type}/{req}/{w}/{pathParamType}", route0)
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
	router := newRuntimeRouter()
	registerRoutes(router)
	addr := resolveListenAddr()
	fmt.Println("tsgodown scaffold listening on", addr)
	server := &http.Server{
		Addr:              addr,
		Handler:           router,
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
	// Extracted path params:
	pathParamType := req.PathValue("type")
	_ = pathParamType
	pathParamReq := req.PathValue("req")
	_ = pathParamReq
	pathParamW := req.PathValue("w")
	_ = pathParamW
	pathParamType2 := req.PathValue("pathParamType")
	_ = pathParamType2

	w.Header().Set("Content-Type", "application/json; charset=utf-8")
	w.Header().Set("X-TSGoDown-Handler", "unknown")
	w.WriteHeader(http.StatusNotImplemented)
	if err := json.NewEncoder(w).Encode(map[string]any{
		"handler": "keywordParams",
		"method": "GET",
		"mode": "unknown",
		"path": "/things/:type/:req/:w/:pathParamType",
	}); err != nil {
		http.Error(w, "json encode failed", http.StatusInternalServerError)
	}
}
