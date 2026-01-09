using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Reflection;
using Croniq.Core;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection;

namespace Croniq.Hosting;

public sealed class CroniqJobAssemblyOptions
{
    public string[] Assemblies { get; set; } = Array.Empty<string>();

    public bool IncludeEntryAssembly { get; set; } = false;
}

public static class JobAssemblyConfigurationExtensions
{
    public static IServiceCollection AddCroniqJobsFromConfiguration(
        this IServiceCollection services,
        IConfiguration configuration,
        string sectionName = "Croniq:Jobs")
    {
        ArgumentNullException.ThrowIfNull(services);
        ArgumentNullException.ThrowIfNull(configuration);

        if (string.IsNullOrWhiteSpace(sectionName))
        {
            throw new ArgumentException("Section name is required.", nameof(sectionName));
        }

        var options = configuration.GetSection(sectionName).Get<CroniqJobAssemblyOptions>() ?? new CroniqJobAssemblyOptions();
        var entries = ResolveAssemblyEntries(configuration, sectionName, options);

        var loaded = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        if (options.IncludeEntryAssembly)
        {
            var entryAssembly = Assembly.GetEntryAssembly();
            if (entryAssembly is null)
            {
                throw new InvalidOperationException("Entry assembly could not be resolved for job registration.");
            }

            RegisterAssembly(services, entryAssembly, loaded);
        }

        foreach (var entry in entries)
        {
            var assembly = LoadAssembly(entry, AppContext.BaseDirectory);
            RegisterAssembly(services, assembly, loaded);
        }

        return services;
    }

    private static IReadOnlyList<string> ResolveAssemblyEntries(
        IConfiguration configuration,
        string sectionName,
        CroniqJobAssemblyOptions options)
    {
        var entries = (options.Assemblies ?? Array.Empty<string>())
            .Where(entry => !string.IsNullOrWhiteSpace(entry))
            .Select(entry => entry.Trim())
            .ToArray();

        if (entries.Length > 0)
        {
            return entries;
        }

        var raw = configuration[$"{sectionName}:Assemblies"];
        if (string.IsNullOrWhiteSpace(raw))
        {
            return Array.Empty<string>();
        }

        return raw
            .Split(new[] { ',', ';', '\n', '\r', '\t' }, StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries)
            .Where(entry => !string.IsNullOrWhiteSpace(entry))
            .ToArray();
    }

    private static void RegisterAssembly(IServiceCollection services, Assembly assembly, HashSet<string> loaded)
    {
        var identity = assembly.FullName
            ?? assembly.GetName().Name
            ?? assembly.Location
            ?? assembly.ToString();

        if (!loaded.Add(identity))
        {
            return;
        }

        services.AddCroniqJobsFromAssembly(assembly);
    }

    private static Assembly LoadAssembly(string descriptor, string baseDirectory)
    {
        if (string.IsNullOrWhiteSpace(descriptor))
        {
            throw new InvalidOperationException("Croniq job assembly entry cannot be empty.");
        }

        var trimmed = descriptor.Trim();

        if (LooksLikePath(trimmed))
        {
            var path = ResolveAssemblyPath(trimmed, baseDirectory);
            if (!File.Exists(path))
            {
                throw new FileNotFoundException($"Croniq job assembly not found at '{path}'.", path);
            }

            return Assembly.LoadFrom(path);
        }

        try
        {
            return Assembly.Load(trimmed);
        }
        catch (Exception)
        {
            var path = ResolveAssemblyPath(trimmed, baseDirectory);
            if (File.Exists(path))
            {
                return Assembly.LoadFrom(path);
            }

            throw new FileNotFoundException($"Croniq job assembly '{trimmed}' could not be resolved. Provide a file path or assembly name.");
        }
    }

    private static bool LooksLikePath(string value)
    {
        return value.EndsWith(".dll", StringComparison.OrdinalIgnoreCase)
            || value.Contains(Path.DirectorySeparatorChar)
            || value.Contains(Path.AltDirectorySeparatorChar);
    }

    private static string ResolveAssemblyPath(string value, string baseDirectory)
    {
        var candidate = Path.IsPathRooted(value)
            ? value
            : Path.Combine(baseDirectory, value);

        if (!Path.HasExtension(candidate))
        {
            candidate += ".dll";
        }

        return Path.GetFullPath(candidate);
    }
}
