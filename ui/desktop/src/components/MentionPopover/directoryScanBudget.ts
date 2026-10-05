export interface DirectoryScanBudget {
  remainingOperations: number;
  remainingResults: number;
  isCancelled: () => boolean;
}
