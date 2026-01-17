using System.Collections.Generic;
using System.Threading;
using System.Threading.Tasks;

namespace Croniq.Persistence.Abstractions;

public interface ICalendarStore
{
    Task<CalendarDefinition?> FindAsync(string calendarId, PartitionScope scope, CancellationToken cancellationToken);

    Task<IReadOnlyCollection<CalendarDefinition>> ListCalendarsAsync(PartitionScope scope, CancellationToken cancellationToken);

    Task UpsertAsync(CalendarUpsert request, CancellationToken cancellationToken);

    Task DeleteAsync(string calendarId, PartitionScope scope, CancellationToken cancellationToken);
}
