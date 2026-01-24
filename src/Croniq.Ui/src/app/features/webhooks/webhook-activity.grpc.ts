import { create, createFileRegistry, type DescMethod, type DescMethodServerStreaming } from '@bufbuild/protobuf';
import { FieldDescriptorProto_Label, FieldDescriptorProto_Type, FileDescriptorProtoSchema } from '@bufbuild/protobuf/wkt';
import type { Transport } from '@connectrpc/connect';
import { createGrpcWebTransport } from '@connectrpc/connect-web';

export type WebhookActivityGrpcStreamRequest = {
    tenantId: string;
    environmentTag?: string;
    fromUtc?: number;
    toUtc?: number;
    updatedSinceUtc?: number;
    hookKeys?: ReadonlyArray<string>;
    jobKeys?: ReadonlyArray<string>;
    limit?: number;
};

export type WebhookActivityGrpcStreamOptions = {
    baseUrl: string;
    headers?: Record<string, string>;
    signal?: AbortSignal;
};

// Inline descriptor keeps gRPC-Web streaming available without code generation.
const WEBHOOK_ACTIVITY_FILE = create(FileDescriptorProtoSchema, {
    name: 'webhook_activity.proto',
    package: 'croniq.rpc',
    syntax: 'proto3',
    messageType: [
        {
            name: 'WebhookActivityStreamRequest',
            field: [
                { name: 'tenant_id', number: 1, label: FieldDescriptorProto_Label.OPTIONAL, type: FieldDescriptorProto_Type.STRING },
                { name: 'environment_tag', number: 2, label: FieldDescriptorProto_Label.OPTIONAL, type: FieldDescriptorProto_Type.STRING },
                { name: 'from_utc', number: 3, label: FieldDescriptorProto_Label.OPTIONAL, type: FieldDescriptorProto_Type.INT64 },
                { name: 'to_utc', number: 4, label: FieldDescriptorProto_Label.OPTIONAL, type: FieldDescriptorProto_Type.INT64 },
                { name: 'updated_since_utc', number: 5, label: FieldDescriptorProto_Label.OPTIONAL, type: FieldDescriptorProto_Type.INT64 },
                { name: 'hook_keys', number: 6, label: FieldDescriptorProto_Label.REPEATED, type: FieldDescriptorProto_Type.STRING },
                { name: 'job_keys', number: 7, label: FieldDescriptorProto_Label.REPEATED, type: FieldDescriptorProto_Type.STRING },
                { name: 'limit', number: 8, label: FieldDescriptorProto_Label.OPTIONAL, type: FieldDescriptorProto_Type.INT32 },
            ],
        },
        {
            name: 'WebhookActivityStreamEvent',
            field: [
                { name: 'type', number: 1, label: FieldDescriptorProto_Label.OPTIONAL, type: FieldDescriptorProto_Type.STRING },
                { name: 'emitted_at_utc', number: 2, label: FieldDescriptorProto_Label.OPTIONAL, type: FieldDescriptorProto_Type.INT64 },
                { name: 'latest_occurred_at_utc', number: 3, label: FieldDescriptorProto_Label.OPTIONAL, type: FieldDescriptorProto_Type.INT64 },
            ],
        },
    ],
    service: [
        {
            name: 'WebhookActivity',
            method: [
                {
                    name: 'Stream',
                    inputType: '.croniq.rpc.WebhookActivityStreamRequest',
                    outputType: '.croniq.rpc.WebhookActivityStreamEvent',
                    clientStreaming: false,
                    serverStreaming: true,
                },
            ],
        },
    ],
});

const WEBHOOK_ACTIVITY_REGISTRY = createFileRegistry(WEBHOOK_ACTIVITY_FILE, () => undefined);
let cachedStreamMethod: DescMethodServerStreaming | null = null;

export function createWebhookActivityGrpcStream(
    request: WebhookActivityGrpcStreamRequest,
    options: WebhookActivityGrpcStreamOptions,
): AsyncIterable<unknown> {
    const method = resolveStreamMethod();
    const transport = createGrpcWebTransport({ baseUrl: options.baseUrl });
    return streamFromTransport(transport, method, request, options);
}

function resolveStreamMethod(): DescMethodServerStreaming {
    if (cachedStreamMethod) {
        return cachedStreamMethod;
    }

    const service = WEBHOOK_ACTIVITY_REGISTRY.getService('croniq.rpc.WebhookActivity');
    if (!service) {
        throw new Error('WebhookActivity gRPC service descriptor not available.');
    }

    const method = service.methods.find((entry) => entry.localName === 'stream' || entry.name === 'Stream');
    if (!method) {
        throw new Error('WebhookActivity/Stream gRPC descriptor not available.');
    }

    if (!isServerStreaming(method)) {
        throw new Error('WebhookActivity/Stream must be server streaming.');
    }

    cachedStreamMethod = method;
    return method;
}

function isServerStreaming(method: DescMethod): method is DescMethodServerStreaming {
    return method.methodKind === 'server_streaming';
}

async function* streamFromTransport(
    transport: Transport,
    method: DescMethodServerStreaming,
    request: WebhookActivityGrpcStreamRequest,
    options: WebhookActivityGrpcStreamOptions,
): AsyncIterable<unknown> {
    const response = await transport.stream(
        method,
        options.signal,
        undefined,
        options.headers,
        singleMessageAsyncIterable(request),
    );

    for await (const message of response.message) {
        yield message;
    }
}

async function* singleMessageAsyncIterable(
    request: WebhookActivityGrpcStreamRequest,
): AsyncIterable<WebhookActivityGrpcStreamRequest> {
    yield request;
}
