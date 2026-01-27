package main

import (
	"context"
	"fmt"
	"log"
	"os"
	"strings"
	"time"

	croniqrunner "github.com/croniq/croniq/sdk/runner-go"
)

func main() {
	baseURL := env("CRONIQ_API_BASEURL", "http://localhost:5080")
	tenantID := env("CRONIQ_TENANT_ID", "default")
	environment := env("CRONIQ_ENVIRONMENT", "dev")
	apiKey := env("CRONIQ_API_KEY", "")

	client, err := croniqrunner.NewClient(croniqrunner.Config{
		BaseURL:        baseURL,
		TenantID:       tenantID,
		EnvironmentTag: environment,
		ApiKey:         apiKey,
	})
	if err != nil {
		log.Fatal(err)
	}

	runnerID := env("CRONIQ_RUNNER_ID", "default")
	batchSize := 1
	waitFor := 25 * time.Second

	log.Printf("Croniq HTTP runner (go)")
	log.Printf("- base_url:    %s", baseURL)
	log.Printf("- tenant_id:   %s", tenantID)
	log.Printf("- environment: %s", environment)
	log.Printf("- runner_id:   %s", runnerID)

	ctx := context.Background()
	for {
		leases, err := client.Poll(ctx, runnerID, batchSize, waitFor)
		if err != nil {
			log.Fatalf("poll failed: %v", err)
		}

		if len(leases) == 0 {
			continue
		}

		for _, lease := range leases {
			log.Printf("claimed lease: jobKey=%s triggerId=%s leaseId=%s", lease.JobKey, lease.TriggerId, lease.LeaseId)
			if lease.ExecutionMode != "" || lease.InvocationSource != "" {
				mode := lease.ExecutionMode
				if mode == "" {
					mode = "normal"
				}
				source := lease.InvocationSource
				if source == "" {
					source = "schedule"
				}
				log.Printf("- intent: mode=%s source=%s", mode, source)
			}

			events := []croniqrunner.WorkEvent{
				{
					Message:   fmt.Sprintf("processing execution %s", lease.ExecutionId),
					Level:     "Information",
					EventType: "runner",
				},
			}
			if err := client.Events(ctx, runnerID, lease, events); err != nil {
				log.Fatalf("events failed: %v", err)
			}

			if err := client.Ack(ctx, runnerID, lease, true, nil, ""); err != nil {
				log.Fatalf("ack failed: %v", err)
			}

			log.Printf("acked lease: leaseId=%s", lease.LeaseId)
		}
	}
}

func env(key, fallback string) string {
	value := strings.TrimSpace(os.Getenv(key))
	if value == "" {
		return fallback
	}
	return value
}
