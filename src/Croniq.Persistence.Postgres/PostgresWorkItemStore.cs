using System;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Data.Postgres;
using Croniq.Data.Postgres.Entities;
using Croniq.Persistence.Abstractions;
using Microsoft.EntityFrameworkCore;

namespace Croniq.Persistence.Postgres;

public sealed class PostgresWorkItemStore : IWorkItemStore
{
    private readonly IDbContextFactory<PostgresDbContext> _dbFactory;

    public PostgresWorkItemStore(IDbContextFactory<PostgresDbContext> dbFactory)
    {
        _dbFactory = dbFactory ?? throw new ArgumentNullException(nameof(dbFactory));
    }

    public async Task UpsertAssignmentAsync(WorkAssignment assignment, CancellationToken cancellationToken)
    {
        if (assignment is null) throw new ArgumentNullException(nameof(assignment));
        if (string.IsNullOrWhiteSpace(assignment.ExecutionId)) throw new ArgumentNullException(nameof(assignment.ExecutionId));
        if (string.IsNullOrWhiteSpace(assignment.JobKey)) throw new ArgumentNullException(nameof(assignment.JobKey));
        if (string.IsNullOrWhiteSpace(assignment.LeaseId)) throw new ArgumentNullException(nameof(assignment.LeaseId));
        if (string.IsNullOrWhiteSpace(assignment.RunnerId)) throw new ArgumentNullException(nameof(assignment.RunnerId));

        var nowUtc = assignment.AssignedAtUtc.UtcDateTime;
        var attempt = assignment.Attempt <= 0 ? 1 : assignment.Attempt;

        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var workItem = await db.WorkItems
            .Include(x => x.Claim)
            .FirstOrDefaultAsync(x => x.ExecutionId == assignment.ExecutionId, cancellationToken)
            .ConfigureAwait(false);

        if (workItem is null)
        {
            workItem = new WorkItemEntity
            {
                ExecutionId = assignment.ExecutionId,
                TenantId = assignment.Scope.TenantId,
                EnvironmentTag = assignment.Scope.EnvironmentTag,
                JobKey = assignment.JobKey,
                TriggerId = assignment.TriggerId,
                Attempt = attempt,
                Status = WorkItemStatus.Leased,
                PayloadJson = assignment.Payload,
                CreatedAtUtc = nowUtc,
                UpdatedAtUtc = nowUtc,
                Claim = new WorkClaimEntity
                {
                    LeaseId = assignment.LeaseId,
                    RunnerId = assignment.RunnerId,
                    LeaseExpiresAtUtc = assignment.LeaseExpiresAtUtc.UtcDateTime,
                    LastHeartbeatAtUtc = nowUtc,
                    CreatedAtUtc = nowUtc,
                    UpdatedAtUtc = nowUtc
                }
            };

            db.WorkItems.Add(workItem);
        }
        else
        {
            workItem.TenantId = assignment.Scope.TenantId;
            workItem.EnvironmentTag = assignment.Scope.EnvironmentTag;
            workItem.JobKey = assignment.JobKey;
            workItem.TriggerId = assignment.TriggerId;
            workItem.Attempt = Math.Max(workItem.Attempt, attempt);
            workItem.Status = WorkItemStatus.Leased;
            workItem.PayloadJson = assignment.Payload;
            workItem.UpdatedAtUtc = nowUtc;

            if (workItem.Claim is null)
            {
                workItem.Claim = new WorkClaimEntity
                {
                    WorkItem = workItem,
                    LeaseId = assignment.LeaseId,
                    RunnerId = assignment.RunnerId,
                    LeaseExpiresAtUtc = assignment.LeaseExpiresAtUtc.UtcDateTime,
                    LastHeartbeatAtUtc = nowUtc,
                    CreatedAtUtc = nowUtc,
                    UpdatedAtUtc = nowUtc
                };
            }
            else
            {
                workItem.Claim.LeaseId = assignment.LeaseId;
                workItem.Claim.RunnerId = assignment.RunnerId;
                workItem.Claim.LeaseExpiresAtUtc = assignment.LeaseExpiresAtUtc.UtcDateTime;
                workItem.Claim.LastHeartbeatAtUtc = nowUtc;
                workItem.Claim.UpdatedAtUtc = nowUtc;
            }
        }

        try
        {
            await db.SaveChangesAsync(cancellationToken).ConfigureAwait(false);
        }
        catch (DbUpdateException)
        {
            if (workItem.WorkItemId != 0)
            {
                throw;
            }

            await UpsertAssignmentRetryAsync(assignment, attempt, nowUtc, cancellationToken).ConfigureAwait(false);
        }
    }

    public async Task<bool> TryRenewAsync(WorkLeaseRenewal renewal, CancellationToken cancellationToken)
    {
        if (renewal is null) throw new ArgumentNullException(nameof(renewal));
        if (string.IsNullOrWhiteSpace(renewal.LeaseId)) throw new ArgumentNullException(nameof(renewal.LeaseId));
        if (string.IsNullOrWhiteSpace(renewal.RunnerId)) throw new ArgumentNullException(nameof(renewal.RunnerId));

        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var claim = await db.WorkClaims
            .Include(x => x.WorkItem)
            .FirstOrDefaultAsync(x => x.LeaseId == renewal.LeaseId, cancellationToken)
            .ConfigureAwait(false);

        if (claim is null || !string.Equals(claim.RunnerId, renewal.RunnerId, StringComparison.OrdinalIgnoreCase))
        {
            return false;
        }

        if (!string.IsNullOrWhiteSpace(renewal.ExecutionId)
            && !string.Equals(claim.WorkItem.ExecutionId, renewal.ExecutionId, StringComparison.OrdinalIgnoreCase))
        {
            return false;
        }

        var nowUtc = renewal.RenewedAtUtc.UtcDateTime;
        claim.LeaseExpiresAtUtc = renewal.LeaseExpiresAtUtc.UtcDateTime;
        claim.LastHeartbeatAtUtc = nowUtc;
        claim.UpdatedAtUtc = nowUtc;
        claim.WorkItem.UpdatedAtUtc = nowUtc;

        await db.SaveChangesAsync(cancellationToken).ConfigureAwait(false);
        return true;
    }

    public async Task<bool> TryCompleteAsync(WorkCompletion completion, CancellationToken cancellationToken)
    {
        if (completion is null) throw new ArgumentNullException(nameof(completion));
        if (string.IsNullOrWhiteSpace(completion.LeaseId)) throw new ArgumentNullException(nameof(completion.LeaseId));
        if (string.IsNullOrWhiteSpace(completion.RunnerId)) throw new ArgumentNullException(nameof(completion.RunnerId));

        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var claim = await db.WorkClaims
            .Include(x => x.WorkItem)
            .FirstOrDefaultAsync(x => x.LeaseId == completion.LeaseId, cancellationToken)
            .ConfigureAwait(false);

        if (claim is null || !string.Equals(claim.RunnerId, completion.RunnerId, StringComparison.OrdinalIgnoreCase))
        {
            return false;
        }

        if (!string.IsNullOrWhiteSpace(completion.ExecutionId)
            && !string.Equals(claim.WorkItem.ExecutionId, completion.ExecutionId, StringComparison.OrdinalIgnoreCase))
        {
            return false;
        }

        var nowUtc = completion.CompletedAtUtc.UtcDateTime;
        claim.WorkItem.Status = ResolveStatus(completion);
        claim.WorkItem.UpdatedAtUtc = nowUtc;
        db.WorkClaims.Remove(claim);

        await db.SaveChangesAsync(cancellationToken).ConfigureAwait(false);
        return true;
    }

    private async Task UpsertAssignmentRetryAsync(
        WorkAssignment assignment,
        int attempt,
        DateTime nowUtc,
        CancellationToken cancellationToken)
    {
        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var workItem = await db.WorkItems
            .Include(x => x.Claim)
            .FirstOrDefaultAsync(x => x.ExecutionId == assignment.ExecutionId, cancellationToken)
            .ConfigureAwait(false);

        if (workItem is null)
        {
            throw new InvalidOperationException($"Work item '{assignment.ExecutionId}' could not be loaded after upsert conflict.");
        }

        workItem.TenantId = assignment.Scope.TenantId;
        workItem.EnvironmentTag = assignment.Scope.EnvironmentTag;
        workItem.JobKey = assignment.JobKey;
        workItem.TriggerId = assignment.TriggerId;
        workItem.Attempt = Math.Max(workItem.Attempt, attempt);
        workItem.Status = WorkItemStatus.Leased;
        workItem.PayloadJson = assignment.Payload;
        workItem.UpdatedAtUtc = nowUtc;

        if (workItem.Claim is null)
        {
            workItem.Claim = new WorkClaimEntity
            {
                WorkItem = workItem,
                LeaseId = assignment.LeaseId,
                RunnerId = assignment.RunnerId,
                LeaseExpiresAtUtc = assignment.LeaseExpiresAtUtc.UtcDateTime,
                LastHeartbeatAtUtc = nowUtc,
                CreatedAtUtc = nowUtc,
                UpdatedAtUtc = nowUtc
            };
        }
        else
        {
            workItem.Claim.LeaseId = assignment.LeaseId;
            workItem.Claim.RunnerId = assignment.RunnerId;
            workItem.Claim.LeaseExpiresAtUtc = assignment.LeaseExpiresAtUtc.UtcDateTime;
            workItem.Claim.LastHeartbeatAtUtc = nowUtc;
            workItem.Claim.UpdatedAtUtc = nowUtc;
        }

        await db.SaveChangesAsync(cancellationToken).ConfigureAwait(false);
    }

    private static string ResolveStatus(WorkCompletion completion)
    {
        if (completion.Succeeded)
        {
            return WorkItemStatus.Succeeded;
        }

        return string.IsNullOrWhiteSpace(completion.DeadLetterReason)
            ? WorkItemStatus.Failed
            : WorkItemStatus.DeadLetter;
    }
}
