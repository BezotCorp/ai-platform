/**
 * Represents the textual form of a valid calendar weekday.
 *
 * `DateDayName` owns weekday-name validation, normalization, lookup, and
 * presentation transformations so weekday strings are not manipulated
 * independently throughout the application.
 */
export class DateDayName {
  private static readonly VALUES: readonly string[] = [
    'sunday',
    'monday',
    'tuesday',
    'wednesday',
    'thursday',
    'friday',
    'saturday',
  ];

  private constructor(private value: string) {}

  /**
   * Creates a `DateDayName` from a native weekday index in the range 0 through 6.
   *
   * @throws {Error} When the index does not represent a valid weekday.
   */
  public static fromIndex(index: number): DateDayName {
    const value: string | undefined = DateDayName.VALUES[index];

    if (value === undefined) {
      throw new Error(`Invalid weekday index: ${index}`);
    }

    return new DateDayName(value);
  }

  /**
   * Creates a `DateDayName` from a weekday string.
   *
   * Leading and trailing whitespace is ignored and matching is case-insensitive.
   *
   * @throws {Error} When the value does not represent a valid weekday.
   */
  public static fromString(value: string): DateDayName {
    return new DateDayName(DateDayName.normalize(value));
  }

  /**
   * Checks whether a string represents a valid weekday name.
   */
  public static isDayName(value: string): boolean {
    return DateDayName.VALUES.includes(value.trim().toLowerCase());
  }

  /**
   * Returns all valid weekday names in their canonical lowercase form.
   */
  public static values(): readonly string[] {
    return DateDayName.VALUES;
  }

  /**
   * Returns all valid weekday names with their first character capitalized.
   */
  public static capitalizedValues(): readonly string[] {
    return DateDayName.VALUES.map(
      (value: string): string => value.charAt(0).toUpperCase() + value.slice(1)
    );
  }

  /**
   * Returns the canonical lowercase weekday name.
   */
  public getName(): string {
    return this.value;
  }

  /**
   * Replaces the weekday name after validating and normalizing the provided value.
   *
   * @throws {Error} When the value does not represent a valid weekday.
   */
  public setName(value: string): void {
    this.value = DateDayName.normalize(value);
  }

  /**
   * Returns the weekday name with its first character capitalized.
   */
  public capitalize(): string {
    return this.value.charAt(0).toUpperCase() + this.value.slice(1);
  }

  /**
   * Returns the weekday name in uppercase.
   */
  public toUpperCase(): string {
    return this.value.toUpperCase();
  }

  /**
   * Returns the weekday name in lowercase.
   */
  public toLowerCase(): string {
    return this.value;
  }

  /**
   * Returns the canonical string representation.
   */
  public toString(): string {
    return this.value;
  }

  private static normalize(value: string): string {
    const normalizedValue: string = value.trim().toLowerCase();

    if (!DateDayName.VALUES.includes(normalizedValue)) {
      throw new Error(`Invalid weekday name: ${value}`);
    }

    return normalizedValue;
  }
}
