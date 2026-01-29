import { splitSseEvents } from './sse';

describe('splitSseEvents', () => {
  it('parses multiple SSE events and preserves fields', () => {
    const payload = [
      'data: hello',
      '',
      'event: activity.updated',
      'data: first',
      'data: second',
      'id: 7',
      '',
      '',
    ].join('\n');

    const result = splitSseEvents(payload);

    expect(result.events).toEqual([
      {
        data: 'hello',
      },
      {
        data: 'first\nsecond',
        event: 'activity.updated',
        id: '7',
      },
    ]);
    expect(result.remainder).toBe('');
  });

  it('returns incomplete fragments as remainder', () => {
    const payload = ['data: ping', '', 'data: partial'].join('\n');

    const result = splitSseEvents(payload);

    expect(result.events).toEqual([
      {
        data: 'ping',
      },
    ]);
    expect(result.remainder).toBe('data: partial');
  });
});
