package main

import (
	"net/http"
	"time"
)

func main() {

	helloHandler := func(w http.ResponseWriter, r *http.Request) {
		time.Sleep(time.Second * 1)

		w.Header().Add("X-Remote-Addr", r.RemoteAddr)
		w.Write([]byte("Hello, world!\n"))
	}

	http.HandleFunc("/", helloHandler)

	http.ListenAndServe("0.0.0.0:5000", nil)

}
