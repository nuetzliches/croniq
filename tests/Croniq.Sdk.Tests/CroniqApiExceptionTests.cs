using System.Net;
using System.Net.Http;
using System.Text;
using System.Threading;
using Croniq.Sdk.Operator;
using Shouldly;
using Xunit;

namespace Croniq.Sdk.Tests;

public sealed class CroniqApiExceptionTests
{
    [Fact]
    public async Task FromResponseAsync_throws_when_response_is_null()
    {
        await Should.ThrowAsync<ArgumentNullException>(
            () => CroniqApiException.FromResponseAsync(null!, CancellationToken.None));
    }

    [Fact]
    public async Task FromResponseAsync_when_content_is_null_uses_default_message_and_null_raw_body()
    {
        using var response = new HttpResponseMessage(HttpStatusCode.BadRequest)
        {
            Content = null
        };

        var ex = await CroniqApiException.FromResponseAsync(response, CancellationToken.None);

        ex.StatusCode.ShouldBe(HttpStatusCode.BadRequest);
        ex.Error.ShouldBeNull();
        ex.RawBody.ShouldBeNull();
        ex.Message.ShouldBe("Croniq API request failed (400 BadRequest).");
    }

    [Fact]
    public async Task FromResponseAsync_when_body_is_not_json_falls_back_to_raw_body_as_message()
    {
        using var response = new HttpResponseMessage(HttpStatusCode.InternalServerError)
        {
            Content = new StringContent("not json", Encoding.UTF8, "application/json")
        };

        var ex = await CroniqApiException.FromResponseAsync(response, CancellationToken.None);

        ex.StatusCode.ShouldBe(HttpStatusCode.InternalServerError);
        ex.Error.ShouldBeNull();
        ex.RawBody.ShouldBe("not json");
        ex.Message.ShouldBe("not json");
    }

    [Fact]
    public async Task FromResponseAsync_when_problem_details_present_prefers_detail_over_title()
    {
        var body = "{\"title\":\"bad_request\",\"detail\":\"validation failed\"}";
        using var response = new HttpResponseMessage(HttpStatusCode.BadRequest)
        {
            Content = new StringContent(body, Encoding.UTF8, "application/problem+json")
        };

        var ex = await CroniqApiException.FromResponseAsync(response, CancellationToken.None);

        ex.StatusCode.ShouldBe(HttpStatusCode.BadRequest);
        ex.Error.ShouldBe("bad_request");
        ex.RawBody.ShouldBe(body);
        ex.Message.ShouldBe("validation failed");
    }

    [Fact]
    public async Task FromResponseAsync_when_problem_details_has_only_title_uses_title_as_message()
    {
        var body = "{\"title\":\"bad_request\"}";
        using var response = new HttpResponseMessage(HttpStatusCode.BadRequest)
        {
            Content = new StringContent(body, Encoding.UTF8, "application/problem+json")
        };

        var ex = await CroniqApiException.FromResponseAsync(response, CancellationToken.None);

        ex.StatusCode.ShouldBe(HttpStatusCode.BadRequest);
        ex.Error.ShouldBe("bad_request");
        ex.RawBody.ShouldBe(body);
        ex.Message.ShouldBe("bad_request");
    }
}
