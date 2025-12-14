## Croniq.Sample.ApiHost

- Swagger dependency note: Swashbuckle.AspNetCore `10.0.1` depends on `Microsoft.OpenApi` `2.3.x`. Upgrading Microsoft.OpenApi to `3.x` causes runtime `MissingMethodException` when serving `/swagger/v1/swagger.json`. Keep `2.3.x` unless you also upgrade Swashbuckle to a version that supports `3.x` and verify swagger output end-to-end.
- Logging: The sample runs without MVC controllers, so ASP.NET emits an info log (“No action descriptors found”) on swagger requests. We override `Microsoft.AspNetCore.Mvc.Infrastructure.DefaultActionDescriptorCollectionProvider` to `Warning` in `appsettings.Development.json` to keep console noise low while preserving important lifecycle logs (job/worker start/stop, retry transitions) at `Information`.
