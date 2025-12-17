package main

import (
	"context"
	"fmt"
	"log"
	"os"
	"time"

	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials/insecure"
	"google.golang.org/grpc/metadata"

	pb "croniq/grpc-client-go"
)

func main() {
	endpoint := getenv("CRONIQ_ENDPOINT", "localhost:5080")
	apiKey := getenv("CRONIQ_API_KEY", "dev-key")
	tenantId := getenv("CRONIQ_TENANT_ID", "1")
	environment := getenv("CRONIQ_ENVIRONMENT", "dev")
	jobKey := getenv("CRONIQ_JOB_KEY", fmt.Sprintf("%s:%s:ops:go-demo", tenantId, environment))

	conn, err := grpc.Dial(endpoint, grpc.WithTransportCredentials(insecure.NewCredentials()))
	if err != nil {
		log.Fatalf("dial failed: %v", err)
	}
	defer conn.Close()

	client := pb.NewSchedulerClient(conn)
	ctx := metadata.AppendToOutgoingContext(context.Background(), "x-croniq-key", apiKey)

	upsert, err := client.UpsertSchedule(ctx, &pb.UpsertScheduleRequest{
		JobKey:         jobKey,
		CronExpression: "0/5 * * * * ?",
		Description:    "go grpc demo",
	})
	if err != nil {
		log.Fatalf("upsert failed: %v", err)
	}
	fmt.Printf("Upserted trigger=%s job=%s\n", upsert.TriggerId, upsert.JobKey)

	trigger, err := client.TriggerJob(ctx, &pb.TriggerJobRequest{JobKey: jobKey})
	if err != nil {
		log.Fatalf("trigger failed: %v", err)
	}
	fmt.Printf("Trigger status=%s\n", trigger.Status)
}

func getenv(key, fallback string) string {
	val := os.Getenv(key)
	if val == "" {
		return fallback
	}
	return val
}
