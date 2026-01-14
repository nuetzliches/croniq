using System.Collections.Generic;

namespace Croniq.Api;

public sealed class CroniqForwardedHeadersOptions
{
    public bool Enabled { get; set; }

    public int ForwardLimit { get; set; } = 1;

    public List<string> KnownNetworks { get; set; } = new();

    public List<string> KnownProxies { get; set; } = new();
}
