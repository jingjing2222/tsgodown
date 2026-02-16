package main

import (
	"fmt"
	"net/http"
)

func main() {
	http.HandleFunc("/health", route0)
	http.HandleFunc("/users", route1)
	fmt.Println("tsgodown scaffold :18081")
	_ = http.ListenAndServe(":18081", nil)
}

func route0(w http.ResponseWriter, req *http.Request) {
	fmt.Fprintln(w, "TODO GET /health -> healthHandler")
}

func route1(w http.ResponseWriter, req *http.Request) {
	fmt.Fprintln(w, "TODO GET /users -> usersHandler")
}
