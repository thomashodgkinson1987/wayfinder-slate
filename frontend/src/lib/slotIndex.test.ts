import { describe, expect, it } from 'vitest';
import { SLOT_INDEX_ERROR, parseSlotIndex } from './slotIndex';

describe('parseSlotIndex', () => {
  it('treats a blank field as "next free"', () => {
    expect(parseSlotIndex('')).toEqual({ ok: true, value: null });
    expect(parseSlotIndex('   ')).toEqual({ ok: true, value: null });
  });

  it('parses whole numbers in range', () => {
    expect(parseSlotIndex('0')).toEqual({ ok: true, value: 0 });
    expect(parseSlotIndex('5')).toEqual({ ok: true, value: 5 });
    expect(parseSlotIndex('255')).toEqual({ ok: true, value: 255 });
    expect(parseSlotIndex(' 12 ')).toEqual({ ok: true, value: 12 });
  });

  it('rejects values outside 0..=255', () => {
    expect(parseSlotIndex('256')).toEqual({ ok: false, error: SLOT_INDEX_ERROR });
    expect(parseSlotIndex('-1')).toEqual({ ok: false, error: SLOT_INDEX_ERROR });
  });

  it('rejects non-integers and non-numeric input', () => {
    for (const raw of ['1.5', '12px', 'abc', '1e3', '+3']) {
      expect(parseSlotIndex(raw)).toEqual({ ok: false, error: SLOT_INDEX_ERROR });
    }
  });
});
