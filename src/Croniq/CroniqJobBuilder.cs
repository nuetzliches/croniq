using System;
using Croniq.Core.Hosting;
using Croniq.Sdk;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.DependencyInjection.Extensions;

namespace Croniq;

public sealed class CroniqJobBuilder
{
    private readonly IServiceCollection _services;
    private readonly CroniqJobAttribute _attribute;

    internal CroniqJobBuilder(IServiceCollection services, CroniqJobAttribute attribute)
    {
        _services = services ?? throw new ArgumentNullException(nameof(services));
        _attribute = attribute ?? throw new ArgumentNullException(nameof(attribute));
    }

    public CroniqJobBuilder AddTrigger(string cronExpression, Action<CroniqTriggerSeedRegistration>? configure = null)
    {
        if (string.IsNullOrWhiteSpace(cronExpression))
        {
            throw new ArgumentException("Cron expression is required.", nameof(cronExpression));
        }

        var registration = new CroniqTriggerSeedRegistration(_attribute, cronExpression);
        configure?.Invoke(registration);

        _services.TryAddEnumerable(ServiceDescriptor.Singleton(registration));
        return this;
    }
}
