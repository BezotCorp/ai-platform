/**
 * @vitest-environment node
 */

import { describe, expect, it } from 'vitest';
import { DateDayName } from '../dateDayName';

describe('DateDayName', () => {
  describe('fromIndex', () => {
    it('creates the expected weekday for every valid native index', () => {
      expect(DateDayName.fromIndex(0).getName()).toBe('sunday');
      expect(DateDayName.fromIndex(1).getName()).toBe('monday');
      expect(DateDayName.fromIndex(2).getName()).toBe('tuesday');
      expect(DateDayName.fromIndex(3).getName()).toBe('wednesday');
      expect(DateDayName.fromIndex(4).getName()).toBe('thursday');
      expect(DateDayName.fromIndex(5).getName()).toBe('friday');
      expect(DateDayName.fromIndex(6).getName()).toBe('saturday');
    });

    it('rejects an index below the valid range', () => {
      expect(() => DateDayName.fromIndex(-1)).toThrow(
        'Invalid weekday index: -1'
      );
    });

    it('rejects an index above the valid range', () => {
      expect(() => DateDayName.fromIndex(7)).toThrow(
        'Invalid weekday index: 7'
      );
    });

    it('rejects a non-integer index', () => {
      expect(() => DateDayName.fromIndex(1.5)).toThrow(
        'Invalid weekday index: 1.5'
      );
    });

    it('rejects a non-number index at runtime', () => {
      expect(() =>
        DateDayName.fromIndex('1' as unknown as number)
      ).toThrow('Invalid weekday index: 1');
    });
  });

  describe('fromString', () => {
    it('creates a weekday name from a canonical value', () => {
      expect(DateDayName.fromString('monday').getName()).toBe('monday');
    });

    it('normalizes case and surrounding whitespace', () => {
      expect(DateDayName.fromString('  MoNdAy  ').getName()).toBe('monday');
    });

    it('rejects an unknown weekday name', () => {
      expect(() => DateDayName.fromString('funday')).toThrow(
        'Invalid weekday name: funday'
      );
    });

    it('rejects a non-string value at runtime', () => {
      expect(() =>
        DateDayName.fromString(1 as unknown as string)
      ).toThrow('Weekday name must be a string');
    });
  });

  describe('isDayName', () => {
    it('returns true for valid weekday names', () => {
      expect(DateDayName.isDayName('sunday')).toBe(true);
      expect(DateDayName.isDayName('MONDAY')).toBe(true);
      expect(DateDayName.isDayName('  Tuesday  ')).toBe(true);
    });

    it('returns false for invalid weekday names', () => {
      expect(DateDayName.isDayName('funday')).toBe(false);
      expect(DateDayName.isDayName('')).toBe(false);
    });

    it('returns false for non-string values at runtime', () => {
      expect(
        DateDayName.isDayName(1 as unknown as string)
      ).toBe(false);
    });
  });

  describe('values', () => {
    it('returns all canonical weekday names in native Date order', () => {
      expect(DateDayName.values()).toEqual([
        'sunday',
        'monday',
        'tuesday',
        'wednesday',
        'thursday',
        'friday',
        'saturday',
      ]);
    });
  });

  describe('capitalizedValues', () => {
    it('returns all weekday names capitalized', () => {
      expect(DateDayName.capitalizedValues()).toEqual([
        'Sunday',
        'Monday',
        'Tuesday',
        'Wednesday',
        'Thursday',
        'Friday',
        'Saturday',
      ]);
    });
  });

  describe('setName', () => {
    it('replaces the value after normalizing it', () => {
      const day = DateDayName.fromString('monday');

      day.setName('  FRIDAY  ');

      expect(day.getName()).toBe('friday');
    });

    it('rejects an invalid replacement value', () => {
      const day = DateDayName.fromString('monday');

      expect(() => day.setName('funday')).toThrow(
        'Invalid weekday name: funday'
      );

      expect(day.getName()).toBe('monday');
    });

    it('rejects a non-string replacement value at runtime', () => {
      const day = DateDayName.fromString('monday');

      expect(() =>
        day.setName(1 as unknown as string)
      ).toThrow('Weekday name must be a string');

      expect(day.getName()).toBe('monday');
    });
  });

  describe('presentation', () => {
    it('returns the canonical value', () => {
      const day = DateDayName.fromString('monday');

      expect(day.getName()).toBe('monday');
      expect(day.toString()).toBe('monday');
      expect(day.toLowerCase()).toBe('monday');
    });

    it('returns capitalized and uppercase representations', () => {
      const day = DateDayName.fromString('monday');

      expect(day.capitalize()).toBe('Monday');
      expect(day.toUpperCase()).toBe('MONDAY');
    });
  });
});
