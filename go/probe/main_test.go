package main

import (
	"strings"
	"testing"
)

func TestFormatSnapshot(t *testing.T) {
	snapshot := &snapshot{
		Scenario: "retry-storm",
		Tick:     7,
		Queue: queueSnapshot{
			Depth: 9,
			Max:   24,
		},
		BackpressureActive: true,
		Totals: totalsSnapshot{
			Generated:       42,
			Accepted:        40,
			Processed:       30,
			Dropped:         2,
			Retried:         5,
			RetryExhausted:  1,
			FailedInService: 6,
		},
		Recent: recentSnapshot{
			Window:            5,
			AvgGenerated:      4.2,
			AvgProcessed:      3.0,
			AvgDropped:        0.4,
			AvgRetried:        1.0,
			BackpressureTicks: 3,
			QueueTrend:        "rising",
		},
		ServiceA: serviceSnapshot{State: "healthy"},
		ServiceB: serviceSnapshot{State: "failed"},
		StatusSignals: []string{
			"queue_rising",
			"retry_activity",
		},
	}

	output := formatSnapshot(snapshot)

	expected := []string{
		"request-pipeline-sim snapshot",
		"scenario: retry-storm",
		"tick: 7",
		"queue: 9/24 backpressure=true",
		"services: A=healthy B=failed",
		"generated=42 accepted=40 processed=30 dropped=2 retried=5 retry_exhausted=1 failed_in_service=6",
		"window=5 avg_generated=4.20 avg_processed=3.00 avg_dropped=0.40 avg_retried=1.00 trend=rising backpressure_ticks=3",
		"signals: queue_rising, retry_activity",
	}

	for _, line := range expected {
		if !strings.Contains(output, line) {
			t.Fatalf("formatted snapshot missing %q:\n%s", line, output)
		}
	}
}

func TestFormatSnapshotWithoutSignals(t *testing.T) {
	snapshot := &snapshot{}
	output := formatSnapshot(snapshot)

	if !strings.Contains(output, "signals: none") {
		t.Fatalf("formatted snapshot did not show empty signals as none:\n%s", output)
	}
}
