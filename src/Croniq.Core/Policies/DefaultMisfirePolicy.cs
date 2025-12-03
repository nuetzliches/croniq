using System;
using Croniq.Persistence.Abstractions;
using Microsoft.Extensions.Options;

namespace Croniq.Core.Policies;

public sealed class DefaultMisfirePolicy : IMisfirePolicy
{
    private readonly MisfirePolicyOptions _options;

    public DefaultMisfirePolicy(IOptions<MisfirePolicyOptions> options)
    {
        _options = options?.Value ?? throw new ArgumentNullException(nameof(options));
    }

    public MisfireDecision Evaluate(TriggerLease lease, MisfirePolicyOptions options, DateTimeOffset now)
    {
        if (lease is null) throw new ArgumentNullException(nameof(lease));
        options ??= _options;

        var delay = now - lease.FireAtUtc;
        if (delay > options.MaxMisfireDelay)
        {
            return new MisfireDecision(true, "misfire-max-delay");
        }

        return new MisfireDecision(false, null);
    }
}
