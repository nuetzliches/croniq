import { create, createFileRegistry, type DescMethod, type DescMethodServerStreaming } from '@bufbuild/protobuf';
import { FieldDescriptorProto_Label, FieldDescriptorProto_Type, FileDescriptorProtoSchema } from '@bufbuild/protobuf/wkt';
import type { Transport } from '@connectrpc/connect';
import { createGrpcWebTransport } from '@connectrpc/connect-web';

export type RunnerPresenceGrpcStreamRequest = {
    tenantId: string;
    environmentTag?: string;
    includeOffline?: boolean;
};

export type RunnerPresenceGrpcStreamOptions = {
    baseUrl: string;
    headers?: Record<string, string>;
    signal?: AbortSignal;
};

// Inline descriptor keeps gRPC-Web streaming available without code generation.
const RUNNER_PRESENCE_FILE = create(FileDescriptorProtoSchema, {
    name: 'runner_presence.proto',
    package: 'croniq.rpc',
    syntax: 'proto3',
    messageType: [
        {
            name: 'RunnerPresenceStreamRequest',
            field: [
                { name: 'tenant_id', number: 1, label: FieldDescriptorProto_Label.OPTIONAL, type: FieldDescriptorProto_Type.STRING },
                { name: 'environment_tag', number: 2, label: FieldDescriptorProto_Label.OPTIONAL, type: FieldDescriptorProto_Type.STRING },
                { name: 'include_offline', number: 3, label: FieldDescriptorProto_Label.OPTIONAL, type: FieldDescriptorProto_Type.BOOL },
            ],
        },
        {
            name: 'RunnerPresenceRunner',
            field: [
                { name: 'runner_id', number: 1, label: FieldDescriptorProto_Label.OPTIONAL, type: FieldDescriptorProto_Type.STRING },
                { name: 'last_seen_at_utc', number: 2, label: FieldDescriptorProto_Label.OPTIONAL, type: FieldDescriptorProto_Type.STRING },
                { name: 'expires_at_utc', number: 3, label: FieldDescriptorProto_Label.OPTIONAL, type: FieldDescriptorProto_Type.STRING },
                { name: 'is_online', number: 4, label: FieldDescriptorProto_Label.OPTIONAL, type: FieldDescriptorProto_Type.BOOL },
                { name: 'metadata_json', number: 5, label: FieldDescriptorProto_Label.OPTIONAL, type: FieldDescriptorProto_Type.STRING },
            ],
        },
        {
            name: 'RunnerPresenceStreamEvent',
            field: [
                { name: 'type', number: 1, label: FieldDescriptorProto_Label.OPTIONAL, type: FieldDescriptorProto_Type.STRING },
                { name: 'emitted_at_utc', number: 2, label: FieldDescriptorProto_Label.OPTIONAL, type: FieldDescriptorProto_Type.INT64 },
                { name: 'latest_seen_at_utc', number: 3, label: FieldDescriptorProto_Label.OPTIONAL, type: FieldDescriptorProto_Type.INT64 },
                { name: 'online_count', number: 4, label: FieldDescriptorProto_Label.OPTIONAL, type: FieldDescriptorProto_Type.INT32 },
                { name: 'total_count', number: 5, label: FieldDescriptorProto_Label.OPTIONAL, type: FieldDescriptorProto_Type.INT32 },
                { name: 'snapshot', number: 6, label: FieldDescriptorProto_Label.REPEATED, type: FieldDescriptorProto_Type.MESSAGE, typeName: '.croniq.rpc.RunnerPresenceRunner' },
                { name: 'updated', number: 7, label: FieldDescriptorProto_Label.REPEATED, type: FieldDescriptorProto_Type.MESSAGE, typeName: '.croniq.rpc.RunnerPresenceRunner' },
                { name: 'removed_runner_ids', number: 8, label: FieldDescriptorProto_Label.REPEATED, type: FieldDescriptorProto_Type.STRING },
            ],
        },
    ],
    service: [
        {
            name: 'RunnerPresence',
            method: [
                {
                    name: 'Stream',
                    inputType: '.croniq.rpc.RunnerPresenceStreamRequest',
                    outputType: '.croniq.rpc.RunnerPresenceStreamEvent',
                    clientStreaming: false,
                    serverStreaming: true,
                },
            ],
        },
    ],
});

const RUNNER_PRESENCE_REGISTRY = createFileRegistry(RUNNER_PRESENCE_FILE, () => undefined);
let cachedStreamMethod: DescMethodServerStreaming | null = null;

export function createRunnerPresenceGrpcStream(
    request: RunnerPresenceGrpcStreamRequest,
    options: RunnerPresenceGrpcStreamOptions,
): AsyncIterable<unknown> {
    const method = resolveStreamMethod();
    const transport = createGrpcWebTransport({ baseUrl: options.baseUrl });
    return streamFromTransport(transport, method, request, options);
}

function resolveStreamMethod(): DescMethodServerStreaming {
    if (cachedStreamMethod) {
        return cachedStreamMethod;
    }

    const service = RUNNER_PRESENCE_REGISTRY.getService('croniq.rpc.RunnerPresence');
    if (!service) {
        throw new Error('RunnerPresence gRPC service descriptor not available.');
    }

    const method = service.methods.find((entry: DescMethod) => {
        const localName = (entry as { localName?: string }).localName;
        return localName === 'stream' || entry.name === 'Stream';
    });
    if (!method) {
        throw new Error('RunnerPresence/Stream gRPC descriptor not available.');
    }

    if (!isServerStreaming(method)) {
        throw new Error('RunnerPresence/Stream must be server streaming.');
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
    request: RunnerPresenceGrpcStreamRequest,
    options: RunnerPresenceGrpcStreamOptions,
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
    request: RunnerPresenceGrpcStreamRequest,
): AsyncIterable<RunnerPresenceGrpcStreamRequest> {
    yield request;
}
