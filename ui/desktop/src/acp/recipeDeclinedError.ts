export class RecipeDeclinedError extends Error {
  constructor() {
    super('Recipe was not trusted by the user');
    this.name = 'RecipeDeclinedError';
  }
}
