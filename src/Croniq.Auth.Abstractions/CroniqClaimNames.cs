namespace Croniq.Auth.Abstractions;

/// <summary>Claim names used by Croniq-issued access tokens.</summary>
public static class CroniqClaimNames
{
    /// <summary>
    /// When <c>true</c>, the authenticated user must change their password before using protected API endpoints.
    /// 
    /// Note: This is intended for password-auth (username/password) flows.
    /// </summary>
    public const string PasswordChangeRequired = "pcr";

    // TODO (2FA): When adding MFA/2FA, consider introducing an explicit claim (or AMR) for MFA state
    // and align enforcement logic to avoid locking users out of 2FA completion flows.
}
