// 09-json-bench (Go): same workload as the Spectra twin.
// 20 manual JSON encode/decode roundtrips per round, 5 rounds.
// Checksums: count = 100, sum_ids = 950, sum_scores = 950.
package main

import (
	"fmt"
	"os"
	"strconv"
	"strings"
)

func main() {
	const perRound = 20
	const iters = 5
	sumIDs := 0
	sumScores := 0
	count := 0
	for it := 0; it < iters; it++ {
		for id := 0; id < perRound; id++ {
			label := "u" + strconv.Itoa(id)
			score := id % 100
			enc := "{\"id\":" + strconv.Itoa(id) + ",\"label\":\"" + label + "\",\"score\":" + strconv.Itoa(score) + "}"
			gotID, gotLabel, gotScore, ok := parse(enc)
			if !ok || gotID != id || gotLabel != label || gotScore != score {
				fmt.Fprintln(os.Stderr, "roundtrip mismatch at id", id)
				os.Exit(1)
			}
			sumIDs += gotID
			sumScores += gotScore
			count++
		}
	}
	if count != 100 || sumIDs != 950 || sumScores != 950 {
		fmt.Fprintln(os.Stderr, "checksum mismatch")
		os.Exit(1)
	}
	fmt.Println("JSON ok objs=100 sum_ids=950")
}

func parse(doc string) (int, string, int, bool) {
	idKey := "\"id\":"
	idStart := strings.Index(doc, idKey)
	if idStart < 0 {
		return 0, "", 0, false
	}
	idRest := doc[idStart+len(idKey):]
	idEnd := strings.Index(idRest, ",")
	if idEnd < 0 {
		return 0, "", 0, false
	}
	id, err := strconv.Atoi(idRest[:idEnd])
	if err != nil {
		return 0, "", 0, false
	}
	labelKey := "\"label\":\""
	labelStart := strings.Index(doc, labelKey)
	if labelStart < 0 {
		return 0, "", 0, false
	}
	labelRest := doc[labelStart+len(labelKey):]
	labelEnd := strings.Index(labelRest, "\"")
	if labelEnd < 0 {
		return 0, "", 0, false
	}
	label := labelRest[:labelEnd]
	scoreKey := "\"score\":"
	scoreStart := strings.Index(doc, scoreKey)
	if scoreStart < 0 {
		return 0, "", 0, false
	}
	score, err := strconv.Atoi(strings.TrimRight(doc[scoreStart+len(scoreKey):], "}"))
	if err != nil {
		return 0, "", 0, false
	}
	return id, label, score, true
}
