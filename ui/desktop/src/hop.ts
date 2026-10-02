export interface Hop {
  status: number;
  statusText: string;
  header(name: string): string | null;
  location: string | null;
}
