package main

import (
	"bufio"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"net"
	"os"
	"strings"
	"time"
)

const (
	defaultAddr = "127.0.0.1:4707"
	rpcTimeout  = 2 * time.Second
)

type request struct {
	Method string `json:"method"`
}

type response struct {
	OK       bool      `json:"ok"`
	Error    string    `json:"error,omitempty"`
	Message  string    `json:"message,omitempty"`
	Snapshot *snapshot `json:"snapshot,omitempty"`
}

type snapshot struct {
	Scenario           string          `json:"scenario"`
	Tick               uint64          `json:"tick"`
	Queue              queueSnapshot   `json:"queue"`
	BackpressureActive bool            `json:"backpressure_active"`
	Totals             totalsSnapshot  `json:"totals"`
	Recent             recentSnapshot  `json:"recent"`
	ServiceA           serviceSnapshot `json:"service_a"`
	ServiceB           serviceSnapshot `json:"service_b"`
	StatusSignals      []string        `json:"status_signals"`
}

type queueSnapshot struct {
	Depth int `json:"depth"`
	Max   int `json:"max"`
}

type totalsSnapshot struct {
	Generated       uint64 `json:"generated"`
	Accepted        uint64 `json:"accepted"`
	Processed       uint64 `json:"processed"`
	Dropped         uint64 `json:"dropped"`
	Retried         uint64 `json:"retried"`
	RetryExhausted  uint64 `json:"retry_exhausted"`
	FailedInService uint64 `json:"failed_in_service"`
}

type recentSnapshot struct {
	Window            int     `json:"window"`
	AvgGenerated      float64 `json:"avg_generated"`
	AvgProcessed      float64 `json:"avg_processed"`
	AvgDropped        float64 `json:"avg_dropped"`
	AvgRetried        float64 `json:"avg_retried"`
	BackpressureTicks int     `json:"backpressure_ticks"`
	QueueTrend        string  `json:"queue_trend"`
}

type serviceSnapshot struct {
	Name                string `json:"name"`
	State               string `json:"state"`
	CapacityPerTick     uint32 `json:"capacity_per_tick"`
	Processed           uint64 `json:"processed"`
	FailedInService     uint64 `json:"failed_in_service"`
	RetryAttempts       uint64 `json:"retry_attempts"`
	RetryExhausted      uint64 `json:"retry_exhausted"`
	LastProcessed       uint32 `json:"last_processed"`
	LastFailedInService uint32 `json:"last_failed_in_service"`
	PressureTicks       uint32 `json:"pressure_ticks"`
	RecoveryTicks       uint32 `json:"recovery_ticks"`
}

func main() {
	addr := flag.String("addr", defaultAddr, "Rust simulation RPC address")
	ping := flag.Bool("ping", false, "check RPC reachability instead of fetching a snapshot")
	flag.Parse()

	if *ping {
		message, err := pingServer(*addr)
		if err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}

		fmt.Println(message)
		return
	}

	snapshot, err := getSnapshot(*addr)
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}

	fmt.Print(formatSnapshot(snapshot))
}

func pingServer(addr string) (string, error) {
	response, err := call(addr, "Ping")
	if err != nil {
		return "", err
	}
	if !response.OK {
		return "", errors.New(response.Error)
	}

	if response.Message == "" {
		return "", errors.New("ping response did not include a message")
	}

	return response.Message, nil
}

func getSnapshot(addr string) (*snapshot, error) {
	response, err := call(addr, "GetSimulationSnapshot")
	if err != nil {
		return nil, err
	}
	if !response.OK {
		return nil, errors.New(response.Error)
	}
	if response.Snapshot == nil {
		return nil, errors.New("snapshot response did not include a snapshot")
	}

	return response.Snapshot, nil
}

func call(addr string, method string) (*response, error) {
	conn, err := net.DialTimeout("tcp", addr, rpcTimeout)
	if err != nil {
		return nil, err
	}
	defer conn.Close()

	if err := conn.SetDeadline(time.Now().Add(rpcTimeout)); err != nil {
		return nil, err
	}

	encoded, err := json.Marshal(request{Method: method})
	if err != nil {
		return nil, err
	}
	encoded = append(encoded, '\n')

	if _, err := conn.Write(encoded); err != nil {
		return nil, err
	}

	line, err := bufio.NewReader(conn).ReadBytes('\n')
	if err != nil {
		return nil, err
	}

	var response response
	if err := json.Unmarshal(line, &response); err != nil {
		return nil, err
	}

	return &response, nil
}

func formatSnapshot(snapshot *snapshot) string {
	signals := "none"
	if len(snapshot.StatusSignals) > 0 {
		signals = strings.Join(snapshot.StatusSignals, ", ")
	}

	return fmt.Sprintf(
		"request-pipeline-sim snapshot\n"+
			"scenario: %s\n"+
			"tick: %d\n"+
			"queue: %d/%d backpressure=%t\n"+
			"services: A=%s B=%s\n"+
			"totals: generated=%d accepted=%d processed=%d dropped=%d retried=%d retry_exhausted=%d failed_in_service=%d\n"+
			"recent: window=%d avg_generated=%.2f avg_processed=%.2f avg_dropped=%.2f avg_retried=%.2f trend=%s backpressure_ticks=%d\n"+
			"signals: %s\n",
		snapshot.Scenario,
		snapshot.Tick,
		snapshot.Queue.Depth,
		snapshot.Queue.Max,
		snapshot.BackpressureActive,
		snapshot.ServiceA.State,
		snapshot.ServiceB.State,
		snapshot.Totals.Generated,
		snapshot.Totals.Accepted,
		snapshot.Totals.Processed,
		snapshot.Totals.Dropped,
		snapshot.Totals.Retried,
		snapshot.Totals.RetryExhausted,
		snapshot.Totals.FailedInService,
		snapshot.Recent.Window,
		snapshot.Recent.AvgGenerated,
		snapshot.Recent.AvgProcessed,
		snapshot.Recent.AvgDropped,
		snapshot.Recent.AvgRetried,
		snapshot.Recent.QueueTrend,
		snapshot.Recent.BackpressureTicks,
		signals,
	)
}
