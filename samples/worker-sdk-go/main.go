package main

import (
	"context"
	"fmt"
	"log"
	"os"
	"strings"
	"time"

	"croniq/worker-sdk-go"
)

func main() {
	baseURL := env("CRONIQ_API_BASEURL", "http://localhost:5080")
	tenantID := env("CRONIQ_TENANT_ID", "default")
	environment := env("CRONIQ_ENVIRONMENT", "dev")
	apiKey := env("CRONIQ_API_KEY", "")

	client, err := croniqworker.NewClient(croniqworker.Config{
		BaseURL:        baseURL,
		TenantID:       tenantID,
		EnvironmentTag: environment,
		ApiKey:         apiKey,
	})
	if err != nil {
		log.Fatal(err)
	}

	runnerID := env("CRONIQ_RUNNER_ID", fmt.Sprintf("go-%s-%d", hostName(), os.Getpid()))
	batchSize := 1
	waitFor := 25 * time.Second

	log.Printf("Croniq HTTP worker (go)")
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

func hostName() string {
	name, err := os.Hostname()
	if err != nil {
		return "unknown"
	}
	return name
}
