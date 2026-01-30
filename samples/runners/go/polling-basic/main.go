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

	jobKey := strings.TrimSpace(os.Getenv("CRONIQ_JOB_KEY"))
	if jobKey == "" {
		jobKey = "samples:go-job"
	}

	config, err := croniqrunner.LoadRunnerConfigFromEnvWithDefaults(croniqrunner.RunnerEnvDefaults{
		RunnerApiKeyEnv:             "CRONIQ_RUNNER_GO_API_KEY",
		DefaultRunnerId:             "default",
		RunnerApiKeyDefaultRunnerId: "go-default",
	})
	if err != nil {
		log.Fatal(err)
	}
	config.HeartbeatInterval = 15 * time.Second

	runner, err := croniqrunner.NewRunner(config)
	if err != nil {
		log.Fatal(err)
	}

	log.Printf("Croniq runner (go)")
	log.Printf("- base_url:    %s", config.BaseURL)
	log.Printf("- tenant_id:   %s", config.TenantID)
	log.Printf("- environment: %s", config.EnvironmentTag)
	log.Printf("- runner_id:   %s", config.RunnerId)
	if config.RunnerInstanceId != "" {
		log.Printf("- runner_instance: %s", config.RunnerInstanceId)
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
