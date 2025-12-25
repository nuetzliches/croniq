namespace Croniq.Options;

public sealed class CroniqJobRegistrySyncOptions
{
    public string Mode { get; set; } = "Off";

    public string ManagedBy { get; set; } = "croniq-job-registry-sync";
}
