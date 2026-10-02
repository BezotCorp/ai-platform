/**
 * Represents the numeric form of a valid calendar weekday.
 *
 * `DateDayNumber` uses the same numbering as JavaScript's native `Date`:
 * Sunday is 0 and Saturday is 6.
 */
export class DateDayNumber {
  private static readonly MIN_VALUE: number = 0;
  private static readonly MAX_VALUE: number = 6;

  private constructor(private value: number) {}

  /**
   * Creates a `DateDayNumber` from a number in the range 0 through 6.
   *
   * @throws {Error} When the number does not represent a valid weekday.
   */
  public static fromNumber(value: number): DateDayNumber {
    DateDayNumber.validate(value);
    return new DateDayNumber(value);
  }

  /**
   * Checks whether a number represents a valid weekday.
   */
  public static isDayNumber(value: number): boolean {
    return (
      Number.isInteger(value) &&
      value >= DateDayNumber.MIN_VALUE &&
      value <= DateDayNumber.MAX_VALUE
    );
  }

  /**
   * Returns the weekday number.
   */
  public getNumber(): number {
    return this.value;
  }

  /**
   * Replaces the weekday number after validating the provided value.
   *
   * @throws {Error} When the number does not represent a valid weekday.
   */
  public setNumber(value: number): void {
    DateDayNumber.validate(value);
    this.value = value;
  }

  private static validate(value: number): void {
    if (!DateDayNumber.isDayNumber(value)) {
      throw new Error(`Invalid weekday number: ${value}`);
    }
  }
}
