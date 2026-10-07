import { computed, ref } from 'vue'
import { useFavoriteJobs, useSetFavorite } from '~/api/queries'
import { describeRefusal } from '~/composables/useActionError'

/**
 * The signed-in user's starred jobs, and the switch that changes them.
 *
 * `available` is false until the list has loaded, and stays false for a
 * session with no user behind it (an API key gets a 403): the stars and the
 * favorite filters are hidden rather than shown and refused on every click.
 */
export function useFavorites() {
  const query = useFavoriteJobs()
  const mutation = useSetFavorite()
  const error = ref<string | null>(null)

  const favorites = computed<ReadonlySet<string>>(() => new Set(query.data.value?.job_keys ?? []))
  const available = computed(() => query.isSuccess.value)

  function isFavorite(jobKey: string): boolean {
    return favorites.value.has(jobKey)
  }

  async function toggle(jobKey: string): Promise<void> {
    error.value = null
    try {
      await mutation.mutateAsync({ jobKey, favorite: !isFavorite(jobKey) })
    } catch (caught) {
      // The cache is already rolled back; say why next to the star.
      error.value = describeRefusal(caught, 'Could not change the favorite.')
    }
  }

  return { favorites, available, isFavorite, toggle, error }
}
