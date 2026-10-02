/**
 * @vitest-environment node
 */

import { describe, expect, it } from 'vitest';
import { DateDayNumber } from '../dateDayNumber';

describe('DateDayNumber', () => {
  describe('fromNumber', () => {
    it('creates every valid native weekday number', () => {
      for (let value = 0; value <= 6; value += 1) {
        expect(DateDayNumber.fromNumber(value).getNumber()).toBe(value);
      }
    });

    it('rejects a value below the valid range', () => {
      expect(() => DateDayNumber.fromNumber(-1)).toThrow(
        'Invalid weekday number: -1'
      );
    });

    it('rejects a value above the valid range', () => {
      expect(() => DateDayNumber.fromNumber(7)).toThrow(
        'Invalid weekday number: 7'
      );
    });

    it('rejects a non-integer value', () => {
      expect(() => DateDayNumber.fromNumber(1.5)).toThrow(
        'Invalid weekday number: 1.5'
      );
    });

    it('rejects NaN', () => {
      expect(() => DateDayNumber.fromNumber(Number.NaN)).toThrow(
        'Invalid weekday number: NaN'
      );
    });

    it('rejects infinity', () => {
      expect(() =>
        DateDayNumber.fromNumber(Number.POSITIVE_INFINITY)
      ).toThrow('Invalid weekday number: Infinity');
    });

    it('rejects a non-number value at runtime', () => {
      expect(() =>
        DateDayNumber.fromNumber('1' as unknown as number)
      ).toThrow('Invalid weekday number: 1');
    });
  });

  describe('isDayNumber', () => {
    it('returns true for every valid weekday number', () => {
      for (let value = 0; value <= 6; value += 1) {
        expect(DateDayNumber.isDayNumber(value)).toBe(true);
      }
    });

    it('returns false for values outside the valid range', () => {
      expect(DateDayNumber.isDayNumber(-1)).toBe(false);
      expect(DateDayNumber.isDayNumber(7)).toBe(false);
    });

    it('returns false for non-integer values', () => {
      expect(DateDayNumber.isDayNumber(1.5)).toBe(false);
      expect(DateDayNumber.isDayNumber(Number.NaN)).toBe(false);
      expect(
        DateDayNumber.isDayNumber(Number.POSITIVE_INFINITY)
      ).toBe(false);
    });

    it('returns false for non-number values at runtime', () => {
      expect(
        DateDayNumber.isDayNumber('1' as unknown as number)
      ).toBe(false);
    });
  });

  describe('setNumber', () => {
    it('replaces the weekday number with another valid value', () => {
      const day = DateDayNumber.fromNumber(1);

      day.setNumber(5);

      expect(day.getNumber()).toBe(5);
    });

    it('rejects an invalid replacement value', () => {
      const day = DateDayNumber.fromNumber(1);

      expect(() => day.setNumber(7)).toThrow(
        'Invalid weekday number: 7'
      );

      expect(day.getNumber()).toBe(1);
    });

    it('rejects a non-number replacement value at runtime', () => {
      const day = DateDayNumber.fromNumber(1);

      expect(() =>
        day.setNumber('5' as unknown as number)
      ).toThrow('Invalid weekday number: 5');

      expect(day.getNumber()).toBe(1);
    });
  });
});
