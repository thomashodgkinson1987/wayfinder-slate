/**
 * Result of parsing the optional slot-number field on the upload form.
 *
 * `value` is `null` for a blank field, meaning "let the server pick the next
 * free slot". Anything else is a validated whole number in `0..=255`.
 */
export type SlotIndexResult =
  | { ok: true; value: number | null }
  | { ok: false; error: string };

/** The message shown whenever a non-blank slot number is out of range. */
export const SLOT_INDEX_ERROR = 'Slot number must be a whole number between 0 and 255.';

/**
 * Parse the raw slot-number text into an optional index.
 *
 * The field is deliberately text so its bound value always stays a string;
 * this helper tolerates stray whitespace and rejects non-integers rather than
 * silently coercing them.
 */
export function parseSlotIndex(raw: string): SlotIndexResult {
  const trimmed = raw.trim();
  if (trimmed === '') {
    return { ok: true, value: null };
  }

  // Reject empty-ish and non-numeric input explicitly: `Number('')` is 0 and
  // `Number('12px')` is NaN, which we don't want to treat as a valid slot.
  if (!/^[0-9]+$/.test(trimmed)) {
    return { ok: false, error: SLOT_INDEX_ERROR };
  }

  const value = Number(trimmed);
  if (!Number.isInteger(value) || value < 0 || value > 255) {
    return { ok: false, error: SLOT_INDEX_ERROR };
  }

  return { ok: true, value };
}
