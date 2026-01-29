package main

import (
	"context"
	"errors"
	"log"
	"os"
	"os/signal"
	"strings"
	"syscall"
	"time"

	croniqrunner "github.com/croniq/croniq/sdk/runner-go"
)

func main() {
	log.SetOutput(os.Stdout)

	baseURL := env("CRONIQ_API_BASEURL", "http://localhost:5080")
	tenantID := env("CRONIQ_TENANT_ID", "default")
	environment := env("CRONIQ_ENVIRONMENT", "dev")
	runnerApiKey := strings.TrimSpace(os.Getenv("CRONIQ_RUNNER_GO_API_KEY"))
	apiKey := strings.TrimSpace(os.Getenv("CRONIQ_API_KEY"))
	if apiKey == "" {
		apiKey = runnerApiKey
	}
	bearerToken := strings.TrimSpace(os.Getenv("CRONIQ_BEARER_TOKEN"))
	runnerID := strings.TrimSpace(os.Getenv("CRONIQ_RUNNER_ID"))
	if runnerID == "" {
		runnerID = "default"
	}
	if runnerApiKey != "" && strings.EqualFold(runnerID, "default") {
		runnerID = "go-default"
	}
	runnerInstanceID := strings.TrimSpace(os.Getenv("CRONIQ_RUNNER_INSTANCE_ID"))
	jobKey := env("CRONIQ_JOB_KEY", "samples:go-job")

	if (apiKey == "" && bearerToken == "") || (apiKey != "" && bearerToken != "") {
		log.Fatal("Set exactly one of CRONIQ_API_KEY or CRONIQ_BEARER_TOKEN")
	}

	runner, err := croniqrunner.NewRunner(croniqrunner.RunnerConfig{
		Config: croniqrunner.Config{
			BaseURL:        baseURL,
			TenantID:       tenantID,
			EnvironmentTag: environment,
			ApiKey:         apiKey,
			BearerToken:    bearerToken,
			Timeout:        60 * time.Second,
		},
		RunnerId:          runnerID,
		RunnerInstanceId:  runnerInstanceID,
		GrpcBaseURL:       env("CRONIQ_GRPC_BASEURL", baseURL),
		TransportMode:     croniqrunner.TransportAuto,
		HeartbeatInterval: 15 * time.Second,
		MaxInflight:       1,
	})
	if err != nil {
		log.Fatal(err)
	}

	log.Printf("Croniq runner (go)")
	log.Printf("- base_url:    %s", baseURL)
	log.Printf("- tenant_id:   %s", tenantID)
	log.Printf("- environment: %s", environment)
	log.Printf("- runner_id:   %s", runnerID)
	if runnerInstanceID != "" {
		log.Printf("- runner_instance: %s", runnerInstanceID)
	}
	log.Printf("- job_key: %s", jobKey)

	runner.OnExecuteWithRegistration(jobKey, func(ctx croniqrunner.ExecutionContext, payload *string, logger croniqrunner.RunnerLogger) error {

		logger.Info("execution started", map[string]any{
			"executionId": ctx.ExecutionId,
			"jobKey":      ctx.JobKey,
			"triggerId":   ctx.TriggerId,
			"mode":        ctx.ExecutionMode,
		})

		if payload != nil && *payload != "" {
			log.Printf("payload received: %s", *payload)
		}

		logger.Info("execution completed", map[string]any{
			"executionId": ctx.ExecutionId,
		})
		return nil
	}, &croniqrunner.RunnerJobRegistration{
		Description: "Demo job registered by the Go runner sample.",
		Metadata: map[string]string{
			"sample": "go",
			"sdk":    "croniq-runner",
		},
	})

	runCtx, cancel := context.WithCancel(context.Background())
	defer cancel()

	signals := make(chan os.Signal, 1)
	signal.Notify(signals, os.Interrupt, syscall.SIGTERM)
	go func() {
		sig := <-signals
		log.Printf("runner draining due to %s", sig.String())
		if err := runner.Drain(30 * time.Second); err != nil {
			log.Printf("drain timeout: %v", err)
		}
		cancel()
	}()

	if err := runner.Run(runCtx); err != nil && !errors.Is(err, context.Canceled) {
		if croniqrunner.IsRunnerIdInUse(err) {
			log.Fatalf("runnerId already in use: %v", err)
		}
		if croniqrunner.IsRunnerJobRegistrationDenied(err) {
			log.Fatalf("job registration denied: %v", err)
		}
		log.Fatalf("runner failed: %v", err)
	}
}

func env(key, fallback string) string {
	value := strings.TrimSpace(os.Getenv(key))
	if value == "" {
		return fallback
	}
	return value
}
