import { DateDayName } from './dateDayName';
import { DateDayNumber } from './dateDayNumber';

/**
 * Application-level wrapper around JavaScript's native `Date`.
 *
 * `AppDate` centralizes date creation, parsing, validation, cloning,
 * serialization, comparison, calendar access, and localized formatting.
 *
 * Serialized values should enter the application through `fromString()`.
 * Existing native `Date` instances can be wrapped through `fromDate()`.
 *
 * Instances are immutable from the caller's perspective: operations that
 * change a date return a new `AppDate`.
 */
export class AppDate {
  private constructor(private readonly value: Date) {}

  /**
   * Creates an `AppDate` representing the current date and time.
   */
  public static now(): AppDate {
    return new AppDate(new Date());
  }

  /**
   * Parses and validates a serialized date value.
   *
   * @throws {Error} When the provided string cannot be parsed as a valid date.
   */
  public static fromString(value: string): AppDate {
    const date: Date = new Date(value);

    if (Number.isNaN(date.getTime())) {
      throw new Error(`Invalid date string: ${value}`);
    }

    return new AppDate(date);
  }

  /**
   * Creates an `AppDate` from an existing native `Date`.
   *
   * The date is cloned so later mutations of the original value cannot affect
   * this instance.
   *
   * @throws {Error} When the provided native date is invalid.
   */
  public static fromDate(value: Date): AppDate {
    const date: Date = new Date(value);

    if (Number.isNaN(date.getTime())) {
      throw new Error('Invalid native Date');
    }

    return new AppDate(date);
  }

  /**
   * Creates an `AppDate` from a Unix timestamp expressed in seconds.
   *
   * @throws {Error} When the timestamp is not finite or cannot produce a valid date.
   */
  public static fromTimestampSeconds(value: number): AppDate {
    if (!Number.isFinite(value)) {
      throw new Error(`Invalid timestamp: ${value}`);
    }

    return AppDate.fromDate(new Date(value * 1000));
  }

  /**
   * Returns a cloned native `Date`.
   *
   * This should primarily be used when crossing a boundary that explicitly
   * requires JavaScript's native `Date` type.
   */
  public toDate(): Date {
    return new Date(this.value);
  }

  /**
   * Serializes the date using JavaScript's ISO 8601 representation.
   */
  public toISOString(): string {
    return this.value.toISOString();
  }

  /**
   * Returns a local calendar key in `YYYY-MM-DD` form.
   *
   * Unlike extracting the date from `toISOString()`, this preserves the local
   * calendar day instead of converting it to UTC first.
   */
  public toLocalDateKey(): string {
    const year: string = String(this.getYear());
    const month: string = String(this.getMonth()).padStart(2, '0');
    const day: string = String(this.getMonthDayNumber()).padStart(2, '0');

    return `${year}-${month}-${day}`;
  }

  /**
   * Formats the date using the native localized date and time formatter.
   */
  public toLocaleString(
    locales?: Intl.LocalesArgument,
    options?: Intl.DateTimeFormatOptions
  ): string {
    return this.value.toLocaleString(locales, options);
  }

  /**
   * Formats the calendar date using the native localized date formatter.
   */
  public toLocaleDateString(
    locales?: Intl.LocalesArgument,
    options?: Intl.DateTimeFormatOptions
  ): string {
    return this.value.toLocaleDateString(locales, options);
  }

  /**
   * Formats the time using the native localized time formatter.
   */
  public toLocaleTimeString(
    locales?: Intl.LocalesArgument,
    options?: Intl.DateTimeFormatOptions
  ): string {
    return this.value.toLocaleTimeString(locales, options);
  }

  /**
   * Returns localized date-time parts for presentation code that needs access
   * to individual formatted components.
   */
  public formatToParts(
    locale: string,
    options: Intl.DateTimeFormatOptions
  ): Intl.DateTimeFormatPart[] {
    return new Intl.DateTimeFormat(locale, options).formatToParts(this.value);
  }

  /**
   * Returns the Unix timestamp in milliseconds.
   */
  public getTime(): number {
    return this.value.getTime();
  }

  /**
   * Returns the local calendar year.
   */
  public getYear(): number {
    return this.value.getFullYear();
  }

  /**
   * Returns the local calendar month using the human-readable range 1 through 12.
   */
  public getMonth(): number {
    return this.value.getMonth() + 1;
  }

  /**
   * Returns the local day number within the month using the range 1 through 31.
   */
  public getMonthDayNumber(): number {
    return this.value.getDate();
  }

  /**
   * Returns the numeric local weekday.
   */
  public getDayNumber(): DateDayNumber {
    return DateDayNumber.fromNumber(this.value.getDay());
  }

  /**
   * Returns the textual local weekday.
   */
  public getDayName(): DateDayName {
    return DateDayName.fromIndex(this.getDayNumber().getNumber());
  }

  /**
   * Returns the local hour using the range 0 through 23.
   */
  public getHours(): number {
    return this.value.getHours();
  }

  /**
   * Returns the local minute using the range 0 through 59.
   */
  public getMinutes(): number {
    return this.value.getMinutes();
  }

  /**
   * Returns a new `AppDate` positioned at the beginning of the same local day.
   */
  public startOfDay(): AppDate {
    const date: Date = this.toDate();
    date.setHours(0, 0, 0, 0);
    return AppDate.fromDate(date);
  }

  /**
   * Returns a new `AppDate` shifted by the provided number of calendar days.
   */
  public addDays(days: number): AppDate {
    if (!Number.isInteger(days)) {
      throw new Error(`Day offset must be an integer: ${days}`);
    }

    const date: Date = this.toDate();
    date.setDate(date.getDate() + days);
    return AppDate.fromDate(date);
  }

  /**
   * Checks whether another date belongs to the same local calendar day.
   */
  public isSameDay(other: AppDate): boolean {
    return (
      this.getYear() === other.getYear() &&
      this.getMonth() === other.getMonth() &&
      this.getMonthDayNumber() === other.getMonthDayNumber()
    );
  }
}
