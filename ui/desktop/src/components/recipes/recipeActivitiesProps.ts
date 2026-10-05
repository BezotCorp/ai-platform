export interface RecipeActivitiesProps {
  append: (text: string) => void;
  activities: string[] | null;
  title?: string;
  parameterValues?: Record<string, string>;
}
