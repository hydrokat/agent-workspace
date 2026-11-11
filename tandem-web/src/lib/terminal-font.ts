/**
 * Computes a font size (in px) that fits `cols` monospace columns within
 * `containerWidthPx`. Clamps to [min, max].
 *
 * Monospace glyphs are approximately 0.6 × font-size wide.
 */
export function mirrorFontSize(
  containerWidthPx: number,
  cols: number,
  min = 7,
  max = 15
): number {
  const CHAR_WIDTH_RATIO = 0.6;
  const size = Math.floor(containerWidthPx / (cols * CHAR_WIDTH_RATIO));
  return Math.max(min, Math.min(max, size));
}
