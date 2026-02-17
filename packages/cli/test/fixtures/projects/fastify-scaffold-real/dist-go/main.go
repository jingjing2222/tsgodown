package main

import (
	"fmt"
	"net/http"
	"os"
)

func resolveListenAddr() string {
	port := os.Getenv("PORT")
	if port == "" {
		port = "8080"
	}
	return ":" + port
}

func main() {
	fmt.Println("tsgodown-fastify-runtime-ready")
	mux := http.NewServeMux()
	mux.HandleFunc("GET /health", func(w http.ResponseWriter, req *http.Request) {
		w.WriteHeader(http.StatusNotImplemented)
		fmt.Fprintln(w, "TODO implement handler health for GET /health")
	})
	_ = http.ListenAndServe(resolveListenAddr(), mux)
}

