/**
 * @vitest-environment node
 */

import { describe, expect, it } from 'vitest';
import { AppDate } from '../appDate';

describe('AppDate', () => {
  describe('fromString', () => {
    it('creates an AppDate from a valid serialized date string', () => {
      const date = AppDate.fromString('2026-01-01T12:30:45.000Z');

      expect(date.toISOString()).toBe('2026-01-01T12:30:45.000Z');
    });

    it('rejects a non-string value at runtime', () => {
      expect(() => AppDate.fromString(1234567890 as unknown as string)).toThrow(
        'Date value must be a string'
      );
    });

    it('rejects an invalid date string', () => {
      expect(() => AppDate.fromString('not-a-date')).toThrow('Invalid date string: not-a-date');
    });
  });

  describe('fromDate', () => {
    it('creates an AppDate from a valid native Date', () => {
      const nativeDate = new Date('2026-01-01T12:30:45.000Z');

      const date = AppDate.fromDate(nativeDate);

      expect(date.toISOString()).toBe('2026-01-01T12:30:45.000Z');
    });

    it('rejects a non-Date value at runtime', () => {
      expect(() => AppDate.fromDate('2026-01-01T12:30:45.000Z' as unknown as Date)).toThrow(
        'Date value must be a native Date'
      );
    });

    it('rejects an invalid native Date', () => {
      expect(() => AppDate.fromDate(new Date(Number.NaN))).toThrow('Invalid native Date');
    });

    it('clones the provided native Date', () => {
      const nativeDate = new Date('2026-01-01T12:30:45.000Z');

      const date = AppDate.fromDate(nativeDate);

      nativeDate.setUTCFullYear(2030);

      expect(date.toISOString()).toBe('2026-01-01T12:30:45.000Z');
    });
  });

  describe('fromTimestampSeconds', () => {
    it('creates an AppDate from a Unix timestamp in seconds', () => {
      const date = AppDate.fromTimestampSeconds(1767268800);

      expect(date.getTime()).toBe(1767268800 * 1000);
    });

    it('rejects a non-number value at runtime', () => {
      expect(() => AppDate.fromTimestampSeconds('1767268800' as unknown as number)).toThrow(
        'Invalid timestamp: 1767268800'
      );
    });

    it('rejects NaN', () => {
      expect(() => AppDate.fromTimestampSeconds(Number.NaN)).toThrow('Invalid timestamp: NaN');
    });

    it('rejects positive infinity', () => {
      expect(() => AppDate.fromTimestampSeconds(Number.POSITIVE_INFINITY)).toThrow(
        'Invalid timestamp: Infinity'
      );
    });

    it('rejects negative infinity', () => {
      expect(() => AppDate.fromTimestampSeconds(Number.NEGATIVE_INFINITY)).toThrow(
        'Invalid timestamp: -Infinity'
      );
    });
  });

  describe('toDate', () => {
    it('returns a clone instead of exposing the internal Date', () => {
      const date = AppDate.fromString('2026-01-01T12:30:45.000Z');

      const nativeDate = date.toDate();
      nativeDate.setUTCFullYear(2030);

      expect(date.toISOString()).toBe('2026-01-01T12:30:45.000Z');
    });
  });

  describe('addDays', () => {
    it('returns a new date without mutating the original', () => {
      const original = AppDate.fromString('2026-01-01T12:30:45.000Z');

      const shifted = original.addDays(1);

      expect(original.toISOString()).toBe('2026-01-01T12:30:45.000Z');
      expect(shifted.toISOString()).toBe('2026-01-02T12:30:45.000Z');
    });

    it('rejects a non-integer offset', () => {
      expect(() => AppDate.fromString('2026-01-01T12:30:45.000Z').addDays(1.5)).toThrow(
        'Day offset must be an integer: 1.5'
      );
    });

    it('rejects a non-number offset at runtime', () => {
      expect(() =>
        AppDate.fromString('2026-01-01T12:30:45.000Z').addDays('1' as unknown as number)
      ).toThrow('Day offset must be an integer: 1');
    });
  });

  describe('isSameDay', () => {
    it('returns true for two times on the same local calendar day', () => {
      const first = AppDate.fromString('2026-01-01T08:00:00');
      const second = AppDate.fromString('2026-01-01T20:00:00');

      expect(first.isSameDay(second)).toBe(true);
    });

    it('returns false for different local calendar days', () => {
      const first = AppDate.fromString('2026-01-01T08:00:00');
      const second = AppDate.fromString('2026-01-02T08:00:00');

      expect(first.isSameDay(second)).toBe(false);
    });
  });
});
