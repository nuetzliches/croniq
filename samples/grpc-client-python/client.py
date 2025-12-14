import os
import grpc
import scheduler_pb2 as pb
import scheduler_pb2_grpc as svc


def main():
    endpoint = os.environ.get("CRONIQ_ENDPOINT", "localhost:5000")
    api_key = os.environ.get("CRONIQ_API_KEY", "dev-key")
    tenant_id = os.environ.get("CRONIQ_TENANT_ID", "1")
    environment_tag = os.environ.get("CRONIQ_ENVIRONMENT", "dev")
    job_key = os.environ.get("CRONIQ_JOB_KEY", f"{tenant_id}:{environment_tag}:samples:python-demo")

    with grpc.insecure_channel(endpoint) as channel:
        client = svc.SchedulerStub(channel)

        metadata = (("x-croniq-key", api_key),)

        try:
            upsert = client.UpsertSchedule(
                pb.UpsertScheduleRequest(
                    job_key=job_key,
                    cron_expression="0/5 * * * * ?",
                    description="python grpc demo",
                ),
                metadata=metadata,
            )
            print(f"Upserted trigger={upsert.trigger_id} job={upsert.job_key}")

            trigger = client.TriggerJob(pb.TriggerJobRequest(job_key=job_key), metadata=metadata)
            print(f"Trigger status={trigger.status}")
        except grpc.RpcError as ex:
            print(f"gRPC error: code={ex.code().name} detail={ex.details()}")


if __name__ == "__main__":
    main()
