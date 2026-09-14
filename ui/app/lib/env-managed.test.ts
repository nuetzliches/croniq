import { describe, expect, it } from 'vitest'
import { declaringKeyVar } from './env-managed'

/**
 * The `default` case is the one that matters. A wrong hint here does not
 * merely fail to help: it tells an operator to add a second declaration of the
 * same client, which the server refuses at its next boot (issue #481). The
 * obvious template produces exactly that wrong answer, so it is pinned.
 */
describe('declaringKeyVar', () => {
  it('names CRONIQ_API_KEY for the default client, not the templated spelling', () => {
    expect(declaringKeyVar('default')).toBe('CRONIQ_API_KEY')
    expect(declaringKeyVar('default')).not.toBe('CRONIQ_API_CLIENT_DEFAULT_KEY')
  })

  it('templates every other name into the client namespace', () => {
    expect(declaringKeyVar('ci')).toBe('CRONIQ_API_CLIENT_CI_KEY')
  })

  it('turns dashes into underscores, since a variable name cannot hold them', () => {
    expect(declaringKeyVar('build-bot')).toBe('CRONIQ_API_CLIENT_BUILD_BOT_KEY')
    expect(declaringKeyVar('eu-west-runner')).toBe('CRONIQ_API_CLIENT_EU_WEST_RUNNER_KEY')
  })
})
