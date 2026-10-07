/**
 * A side pane's width, kept between `min` and `max`.
 *
 * `max` comes from the space left beside the pane and can drop below `min` in
 * a narrow window. The pane then gets `min` and the list beside it gives way,
 * rather than the pane collapsing to nothing.
 */
export function clampPaneWidth(width: number, min: number, max: number): number {
  return Math.min(Math.max(width, min), Math.max(min, max))
}
